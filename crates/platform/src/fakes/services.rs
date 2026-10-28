//! Queues, Workers AI, Vectorize, Email Sending, rate limits, Analytics Engine and `waitUntil`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::ai::{Ai, AiOptions, MarkdownInput, MarkdownOutput};
use crate::background::Background;
use crate::email::{MailSender, SendError, StructuredEmail};
use crate::metrics::{DataPoint, Metrics};
use crate::queues::{Incoming, QueueName, QueueProducer, Settle, encode};
use crate::ratelimit::{RateLimiter, RlBucket};
use crate::vectorize::{
    IndexInfo, VectorIndex, VectorMatch, VectorRecord, check_upsert, query_options,
};
use crate::{PResult, PlatformError, PlatformErrorKind};

// ---- Queues ----------------------------------------------------------------------------------------

/// Captured sends: `(queue, body, delay)`.
#[derive(Default)]
pub struct FakeQueues {
    sent: RefCell<Vec<(QueueName, Value, u32)>>,
}

impl FakeQueues {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sent(&self) -> Vec<(QueueName, Value, u32)> {
        self.sent.borrow().clone()
    }

    /// Removes and returns the bodies sent to `q`.
    pub fn take(&self, q: QueueName) -> Vec<Value> {
        let mut sent = self.sent.borrow_mut();
        let (taken, kept): (Vec<_>, Vec<_>) = sent.drain(..).partition(|(n, _, _)| *n == q);
        *sent = kept;
        taken.into_iter().map(|(_, body, _)| body).collect()
    }

    fn record(&self, q: QueueName, texts: Vec<String>, delay_s: u32) {
        let mut sent = self.sent.borrow_mut();
        for t in texts {
            sent.push((q, serde_json::from_str(&t).expect("encoded JSON"), delay_s));
        }
    }
}

impl QueueProducer for FakeQueues {
    async fn send<T: Serialize>(&self, q: QueueName, body: &T, delay_s: u32) -> PResult<()> {
        let texts = encode(q, std::slice::from_ref(body), delay_s)?;
        self.record(q, texts, delay_s);
        Ok(())
    }

    async fn send_batch<T: Serialize>(
        &self,
        q: QueueName,
        bodies: &[T],
        delay_s: u32,
    ) -> PResult<()> {
        let texts = encode(q, bodies, delay_s)?;
        self.record(q, texts, delay_s);
        Ok(())
    }
}

/// How a received message was settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    Pending,
    Acked,
    Retried(Option<u32>),
}

struct SettleLog(Rc<Cell<Settled>>);

impl Settle for SettleLog {
    fn ack(&self) {
        self.0.set(Settled::Acked);
    }

    fn retry(&self, delay_s: Option<u32>) {
        self.0.set(Settled::Retried(delay_s));
    }
}

/// A delivered message and the cell that records how the consumer settled it.
pub fn incoming<T>(
    id: &str,
    timestamp_ms: i64,
    attempts: u32,
    body: T,
) -> (Incoming<T>, Rc<Cell<Settled>>) {
    let log = Rc::new(Cell::new(Settled::Pending));
    let msg = Incoming::new(
        id.to_owned(),
        timestamp_ms,
        attempts,
        body,
        Rc::new(SettleLog(log.clone())),
    );
    (msg, log)
}

// ---- Workers AI ------------------------------------------------------------------------------------

/// Answers scripted per model, in order; every call recorded as `(model, input, options)`.
#[derive(Default)]
pub struct FakeAi {
    answers: RefCell<HashMap<String, VecDeque<PResult<Value>>>>,
    bytes: RefCell<HashMap<String, VecDeque<PResult<Vec<u8>>>>>,
    markdown: RefCell<VecDeque<PResult<Vec<MarkdownOutput>>>>,
    calls: RefCell<Vec<(String, Value, AiOptions)>>,
}

impl FakeAi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn answer(&self, model: &str, answer: PResult<Value>) {
        self.answers
            .borrow_mut()
            .entry(model.to_owned())
            .or_default()
            .push_back(answer);
    }

    pub fn answer_bytes(&self, model: &str, answer: PResult<Vec<u8>>) {
        self.bytes
            .borrow_mut()
            .entry(model.to_owned())
            .or_default()
            .push_back(answer);
    }

    pub fn answer_markdown(&self, answer: PResult<Vec<MarkdownOutput>>) {
        self.markdown.borrow_mut().push_back(answer);
    }

    pub fn calls(&self) -> Vec<(String, Value, AiOptions)> {
        self.calls.borrow().clone()
    }

    fn record<I: Serialize>(&self, model: &str, input: &I, opts: &AiOptions) -> PResult<()> {
        let input = serde_json::to_value(input)
            .map_err(|e| PlatformError::invalid("AI", format!("input: {e}")))?;
        self.calls
            .borrow_mut()
            .push((model.to_owned(), input, opts.clone()));
        Ok(())
    }
}

fn unscripted(model: &str) -> PlatformError {
    PlatformError::unavailable("AI", format!("no scripted answer for {model}"))
}

impl Ai for FakeAi {
    async fn run<I: Serialize, O: DeserializeOwned>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<O> {
        self.record(model, input, opts)?;
        let answer = self
            .answers
            .borrow_mut()
            .get_mut(model)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| unscripted(model))??;
        serde_json::from_value(answer).map_err(|e| {
            PlatformError::new(PlatformErrorKind::Corrupt, "AI", format!("output: {e}"))
        })
    }

    async fn run_bytes<I: Serialize>(
        &self,
        model: &str,
        input: &I,
        opts: &AiOptions,
    ) -> PResult<Vec<u8>> {
        self.record(model, input, opts)?;
        self.bytes
            .borrow_mut()
            .get_mut(model)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| unscripted(model))?
    }

    async fn to_markdown(&self, docs: &[MarkdownInput]) -> PResult<Vec<MarkdownOutput>> {
        let names: Vec<&str> = docs.iter().map(|d| d.name.as_str()).collect();
        self.record("toMarkdown", &names, &AiOptions::default())?;
        self.markdown
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| unscripted("toMarkdown"))?
    }
}

// ---- Vectorize -------------------------------------------------------------------------------------

/// An index with cosine similarity, namespaces and equality filters on metadata (`{k: v}`,
/// `{k: {"$eq": v}}`, `{k: {"$ne": v}}`, `{k: {"$in": [..]}}`). Mutations are visible at once.
#[derive(Default)]
pub struct FakeVectors {
    records: RefCell<BTreeMap<String, VectorRecord>>,
    mutations: Cell<u64>,
    dimensions: Cell<u32>,
}

impl FakeVectors {
    pub fn new() -> Self {
        let v = Self::default();
        v.dimensions.set(1024);
        v
    }

    fn mutation(&self) -> String {
        self.mutations.set(self.mutations.get() + 1);
        format!("mut-{}", self.mutations.get())
    }
}

fn matches_filter(meta: &serde_json::Map<String, Value>, filter: &Value) -> bool {
    let Some(filter) = filter.as_object() else {
        return true;
    };
    filter.iter().all(|(k, cond)| {
        let v = meta.get(k).unwrap_or(&Value::Null);
        match cond {
            Value::Object(ops) => ops.iter().all(|(op, arg)| match op.as_str() {
                "$eq" => v == arg,
                "$ne" => v != arg,
                "$in" => arg.as_array().is_some_and(|a| a.contains(v)),
                "$nin" => arg.as_array().is_some_and(|a| !a.contains(v)),
                "$gte" | "$gt" | "$lte" | "$lt" => match (v.as_f64(), arg.as_f64()) {
                    (Some(x), Some(y)) => match op.as_str() {
                        "$gte" => x >= y,
                        "$gt" => x > y,
                        "$lte" => x <= y,
                        _ => x < y,
                    },
                    _ => false,
                },
                _ => false,
            }),
            other => v == other,
        }
    })
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

impl VectorIndex for FakeVectors {
    async fn upsert(&self, vectors: &[VectorRecord]) -> PResult<String> {
        check_upsert("VECTORS", vectors)?;
        let mut records = self.records.borrow_mut();
        for v in vectors {
            records.insert(v.id.clone(), v.clone());
        }
        Ok(self.mutation())
    }

    async fn query(
        &self,
        namespace: &str,
        vector: &[f32],
        top_k: u32,
        filter: &Value,
    ) -> PResult<Vec<VectorMatch>> {
        query_options("VECTORS", namespace, top_k, filter)?;
        let mut out: Vec<VectorMatch> = self
            .records
            .borrow()
            .values()
            .filter(|r| r.namespace == namespace && matches_filter(&r.metadata, filter))
            .map(|r| VectorMatch {
                id: r.id.clone(),
                score: cosine(vector, &r.values),
            })
            .collect();
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        out.truncate(top_k as usize);
        Ok(out)
    }

    async fn query_by_id(
        &self,
        namespace: &str,
        id: &str,
        top_k: u32,
        filter: &Value,
    ) -> PResult<Vec<VectorMatch>> {
        let values = self.records.borrow().get(id).map(|r| r.values.clone());
        match values {
            Some(v) => self.query(namespace, &v, top_k, filter).await,
            None => Ok(Vec::new()),
        }
    }

    async fn delete_by_ids(&self, ids: &[String]) -> PResult<String> {
        let mut records = self.records.borrow_mut();
        for id in ids {
            records.remove(id);
        }
        Ok(self.mutation())
    }

    async fn get_by_ids(&self, ids: &[String]) -> PResult<Vec<String>> {
        let records = self.records.borrow();
        Ok(ids
            .iter()
            .filter(|id| records.contains_key(*id))
            .cloned()
            .collect())
    }

    async fn describe(&self) -> PResult<IndexInfo> {
        Ok(IndexInfo {
            vector_count: self.records.borrow().len() as u64,
            dimensions: self.dimensions.get(),
        })
    }
}

// ---- Email Sending ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SentMail {
    Structured(StructuredEmail),
    Raw {
        from: String,
        to: String,
        mime: Vec<u8>,
    },
}

/// Records every send; answers with scripted outcomes, then `Ok("fake-{n}")`.
#[derive(Default)]
pub struct FakeMail {
    sent: RefCell<Vec<SentMail>>,
    outcomes: RefCell<VecDeque<Result<String, SendError>>>,
}

impl FakeMail {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn outcome(&self, outcome: Result<String, SendError>) {
        self.outcomes.borrow_mut().push_back(outcome);
    }

    pub fn sent(&self) -> Vec<SentMail> {
        self.sent.borrow().clone()
    }

    fn next(&self, mail: SentMail) -> Result<String, SendError> {
        self.sent.borrow_mut().push(mail);
        let n = self.sent.borrow().len();
        self.outcomes
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(format!("fake-{n}")))
    }
}

impl MailSender for FakeMail {
    async fn send(&self, msg: &StructuredEmail, _deadline_ms: u32) -> Result<String, SendError> {
        self.next(SentMail::Structured(msg.clone()))
    }

    async fn send_raw(
        &self,
        from: &str,
        to: &str,
        mime: &[u8],
        _deadline_ms: u32,
    ) -> Result<String, SendError> {
        self.next(SentMail::Raw {
            from: from.to_owned(),
            to: to.to_owned(),
            mime: mime.to_vec(),
        })
    }
}

// ---- Rate limits -----------------------------------------------------------------------------------

/// Counts per bucket and key against each bucket's limit; [`FakeLimits::reset`] ends the window.
#[derive(Default)]
pub struct FakeLimits {
    counts: RefCell<HashMap<(RlBucket, String), u32>>,
    limits: RefCell<HashMap<RlBucket, u32>>,
}

impl FakeLimits {
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides a bucket's limit for this test.
    pub fn set_limit(&self, bucket: RlBucket, limit: u32) {
        self.limits.borrow_mut().insert(bucket, limit);
    }

    pub fn reset(&self) {
        self.counts.borrow_mut().clear();
    }
}

impl RateLimiter for FakeLimits {
    async fn allow(&self, bucket: RlBucket, key: &str) -> PResult<bool> {
        let limit = self
            .limits
            .borrow()
            .get(&bucket)
            .copied()
            .unwrap_or(bucket.limit());
        let mut counts = self.counts.borrow_mut();
        let n = counts.entry((bucket, key.to_owned())).or_default();
        *n += 1;
        Ok(*n <= limit)
    }
}

// ---- Analytics Engine and waitUntil ----------------------------------------------------------------

#[derive(Default)]
pub struct FakeMetrics {
    points: RefCell<Vec<DataPoint>>,
}

impl FakeMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn points(&self) -> Vec<DataPoint> {
        self.points.borrow().clone()
    }
}

impl Metrics for FakeMetrics {
    fn write(&self, point: DataPoint) {
        self.points.borrow_mut().push(point);
    }
}

/// Futures handed to `waitUntil`, run by [`FakeBackground::run_pending`].
#[derive(Default)]
pub struct FakeBackground {
    tasks: RefCell<Vec<Pin<Box<dyn Future<Output = ()>>>>>,
}

impl FakeBackground {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> usize {
        self.tasks.borrow().len()
    }

    /// Runs every queued future to completion, including those they queue.
    pub fn run_pending(&self) {
        loop {
            let tasks: Vec<_> = self.tasks.borrow_mut().drain(..).collect();
            if tasks.is_empty() {
                return;
            }
            for t in tasks {
                super::block_on(t);
            }
        }
    }
}

impl Background for FakeBackground {
    fn wait_until(&self, fut: impl Future<Output = ()> + 'static) {
        self.tasks.borrow_mut().push(Box::pin(fut));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::fakes::block_on;

    #[test]
    fn queues_capture_and_settle() {
        let q = FakeQueues::new();
        block_on(q.send(QueueName::Index, &json!({ "op": "embed" }), 30)).unwrap();
        block_on(q.send_batch(QueueName::Webhooks, &[1, 2], 0)).unwrap();
        assert!(block_on(q.send(QueueName::Index, &1, 86_401)).is_err());
        assert_eq!(q.take(QueueName::Webhooks), [json!(1), json!(2)]);
        assert_eq!(q.sent(), [(QueueName::Index, json!({ "op": "embed" }), 30)]);

        let (msg, settled) = incoming("m1", 5, 1, json!({ "x": 1 }));
        msg.retry(Some(120));
        assert_eq!(settled.get(), Settled::Retried(Some(120)));
        #[derive(serde::Deserialize)]
        struct Body {
            x: u32,
        }
        let typed = msg.decode::<Body>().map_err(|(_, e)| e).unwrap();
        assert_eq!(typed.body.x, 1);
        typed.ack();
        assert_eq!(settled.get(), Settled::Acked);
    }

    #[test]
    fn ai_vectors_mail_limits_metrics_background() {
        let ai = FakeAi::new();
        ai.answer("@cf/baai/bge-m3", Ok(json!({ "data": [[0.5]] })));
        let out: Value = block_on(ai.run(
            "@cf/baai/bge-m3",
            &json!({ "text": ["a"] }),
            &AiOptions::content(None),
        ))
        .unwrap();
        assert_eq!(out["data"][0][0], 0.5);
        assert!(
            block_on(ai.run::<_, Value>("@cf/baai/bge-m3", &1, &AiOptions::default())).is_err()
        );
        assert!(!ai.calls()[0].2.collect_log);

        let v = FakeVectors::new();
        let rec = |id: &str, ns: &str, values: Vec<f32>, tenant: &str| VectorRecord {
            id: id.into(),
            namespace: ns.into(),
            values,
            metadata: json!({ "tenant": tenant }).as_object().unwrap().clone(),
        };
        block_on(v.upsert(&[
            rec("a", "n1", vec![1.0, 0.0], "t1"),
            rec("b", "n1", vec![0.7, 0.7], "t1"),
            rec("c", "n1", vec![1.0, 0.0], "t2"),
            rec("d", "n2", vec![1.0, 0.0], "t1"),
        ]))
        .unwrap();
        let hits = block_on(v.query("n1", &[1.0, 0.0], 5, &json!({ "tenant": "t1" }))).unwrap();
        assert_eq!(
            hits.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        block_on(v.delete_by_ids(&["a".into()])).unwrap();
        assert_eq!(
            block_on(v.get_by_ids(&["a".into(), "b".into()])).unwrap(),
            ["b"]
        );
        assert_eq!(block_on(v.describe()).unwrap().vector_count, 3);

        let mail = FakeMail::new();
        mail.outcome(Err(SendError::Timeout));
        assert_eq!(
            block_on(mail.send_raw("a@x.test", "b@y.test", b"MIME", 30_000)),
            Err(SendError::Timeout)
        );
        assert_eq!(
            block_on(mail.send(&StructuredEmail::default(), 30_000)),
            Ok("fake-2".into())
        );
        assert_eq!(mail.sent().len(), 2);

        let rl = FakeLimits::new();
        rl.set_limit(RlBucket::SignIn, 2);
        let tries: Vec<bool> = (0..3)
            .map(|_| block_on(rl.allow(RlBucket::SignIn, "ip")).unwrap())
            .collect();
        assert_eq!(tries, [true, true, false]);
        assert!(block_on(rl.allow(RlBucket::SignIn, "other")).unwrap());
        rl.reset();
        assert!(block_on(rl.allow(RlBucket::SignIn, "ip")).unwrap());

        let m = FakeMetrics::new();
        m.write(DataPoint {
            indexes: ["t".into()],
            blobs: vec![],
            doubles: vec![1.0],
        });
        assert_eq!(m.points().len(), 1);

        let bg = FakeBackground::new();
        let ran = Rc::new(Cell::new(0));
        let r = ran.clone();
        bg.wait_until(async move { r.set(r.get() + 1) });
        assert_eq!((bg.pending(), ran.get()), (1, 0));
        bg.run_pending();
        assert_eq!((bg.pending(), ran.get()), (0, 1));
    }
}
