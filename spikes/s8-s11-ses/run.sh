#!/usr/bin/env bash
# Spikes S8 and S11 (build plan M1), run by .github/workflows/spikes-ses.yml with the staging role's
# temporary credentials in the environment (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN).
#
#   run.sh setup     create the SES identity, bucket, topic, backstop queue and receipt rules; print the
#                    DKIM tokens (the DNS records are published separately, in the Cloudflare zone)
#   run.sh exercise  send and receive through SES from the spike Worker in workerd, verify the SNS
#                    notifications, fetch the stored messages from S3, then remove everything
#   run.sh cleanup   remove everything (also the last step of `exercise`)
#
# Every resource is named pi-spike-11-* (deploy/aws/spikes-policy.json). Nothing secret is printed.
set -euo pipefail

PHASE=${1:?setup, exercise or cleanup}
R=eu-west-2
D=pi-spike-11.pylotamail.com
ACCT=$(aws sts get-caller-identity --query Account --output text)
BUCKET=pi-spike-11-$ACCT
TOPIC=pi-spike-11-inbound
QUEUE=pi-spike-11-backstop
RS=pi-spike-11
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$HERE/results.json
export AWS_REGION=$R AWS_DEFAULT_REGION=$R

topic_arn() { echo "arn:aws:sns:$R:$ACCT:$TOPIC"; }
queue_url() { aws sqs get-queue-url --queue-name "$QUEUE" --query QueueUrl --output text; }

setup() {
  local active
  active=$(aws ses describe-active-receipt-rule-set --query Metadata.Name --output text 2>/dev/null || true)
  if [ -n "$active" ] && [ "$active" != "None" ] && [ "$active" != "$RS" ]; then
    echo "another receipt rule set is active ($active): one per region, stopping" >&2
    exit 1
  fi

  aws sesv2 get-email-identity --email-identity "$D" >/dev/null 2>&1 ||
    aws sesv2 create-email-identity --email-identity "$D" \
      --tags Key=pylota-identity:api-host,Value=pi-spike-11 >/dev/null

  aws s3api head-bucket --bucket "$BUCKET" 2>/dev/null ||
    aws s3api create-bucket --bucket "$BUCKET" --create-bucket-configuration LocationConstraint=$R >/dev/null
  aws s3api put-public-access-block --bucket "$BUCKET" --public-access-block-configuration \
    BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
  aws s3api put-bucket-policy --bucket "$BUCKET" --policy "$(jq -nc --arg b "$BUCKET" --arg a "$ACCT" --arg r "$R" --arg rs "$RS" '{
    Version: "2012-10-17",
    Statement: [{ Sid: "SesWritesInbound", Effect: "Allow", Principal: { Service: "ses.amazonaws.com" },
      Action: "s3:PutObject", Resource: "arn:aws:s3:::\($b)/in/*",
      Condition: { StringEquals: { "aws:SourceAccount": $a },
                   ArnLike: { "aws:SourceArn": "arn:aws:ses:\($r):\($a):receipt-rule-set/\($rs):receipt-rule/*" } } }] }')"

  aws sns create-topic --name "$TOPIC" --attributes SignatureVersion=2 >/dev/null
  aws sns set-topic-attributes --topic-arn "$(topic_arn)" --attribute-name Policy --attribute-value \
    "$(jq -nc --arg t "$(topic_arn)" --arg a "$ACCT" '{ Version: "2012-10-17", Statement: [
      { Sid: "SesPublishes", Effect: "Allow", Principal: { Service: "ses.amazonaws.com" }, Action: "sns:Publish",
        Resource: $t, Condition: { StringEquals: { "aws:SourceAccount": $a } } }] }')"

  aws sqs create-queue --queue-name "$QUEUE" --attributes MessageRetentionPeriod=1209600,SqsManagedSseEnabled=true >/dev/null
  local qarn
  qarn="arn:aws:sqs:$R:$ACCT:$QUEUE"
  aws sqs set-queue-attributes --queue-url "$(queue_url)" --attributes "$(jq -nc --arg q "$qarn" --arg t "$(topic_arn)" '{
    Policy: ({ Version: "2012-10-17", Statement: [{ Sid: "TopicSends", Effect: "Allow",
      Principal: { Service: "sns.amazonaws.com" }, Action: "sqs:SendMessage", Resource: $q,
      Condition: { ArnEquals: { "aws:SourceArn": $t } } }] } | tostring) }')"
  aws sns subscribe --topic-arn "$(topic_arn)" --protocol sqs --notification-endpoint "$qarn" >/dev/null

  aws ses describe-receipt-rule-set --rule-set-name "$RS" >/dev/null 2>&1 ||
    aws ses create-receipt-rule-set --rule-set-name "$RS"
  aws ses describe-receipt-rule --rule-set-name "$RS" --rule-name retired >/dev/null 2>&1 ||
    aws ses create-receipt-rule --rule-set-name "$RS" --rule "$(jq -nc --arg d "$D" '{
      Name: "retired", Enabled: true, Recipients: ["retired@\($d)"], ScanEnabled: true, TlsPolicy: "Optional",
      Actions: [{ BounceAction: { SmtpReplyCode: "550", StatusCode: "5.1.1",
                  Message: "This address no longer accepts mail", Sender: "mailer-daemon@\($d)" } },
                { StopAction: { Scope: "RuleSet" } }] }')"
  aws ses describe-receipt-rule --rule-set-name "$RS" --rule-name pm-deliver >/dev/null 2>&1 ||
    aws ses create-receipt-rule --rule-set-name "$RS" --after retired --rule "$(jq -nc --arg b "$BUCKET" --arg t "$(topic_arn)" '{
      Name: "pm-deliver", Enabled: true, ScanEnabled: true, TlsPolicy: "Optional",
      Actions: [{ S3Action: { BucketName: $b, ObjectKeyPrefix: "in/", TopicArn: $t } }] }')"
  aws ses set-active-receipt-rule-set --rule-set-name "$RS"

  echo "== DKIM tokens for $D (publish as <token>._domainkey.$D CNAME <token>.dkim.amazonses.com)"
  aws sesv2 get-email-identity --email-identity "$D" --query 'DkimAttributes.Tokens' --output text
  echo "== MX for $D: 10 inbound-smtp.$R.amazonaws.com"
  echo "setup: ok"
}

cleanup() {
  set +e
  if [ "$(aws ses describe-active-receipt-rule-set --query Metadata.Name --output text 2>/dev/null)" = "$RS" ]; then
    aws ses set-active-receipt-rule-set
  fi
  aws ses delete-receipt-rule-set --rule-set-name "$RS"
  aws s3 rm "s3://$BUCKET/in/" --recursive --quiet
  aws s3api delete-bucket --bucket "$BUCKET"
  aws sqs delete-queue --queue-url "$(queue_url)"
  aws sns delete-topic --topic-arn "$(topic_arn)"
  aws sesv2 delete-email-identity --email-identity "$D"
  echo "cleanup: done"
  set -e
}

exercise() {
  trap cleanup EXIT
  for _ in $(seq 1 60); do
    [ "$(aws sesv2 get-email-identity --email-identity "$D" --query VerifiedForSendingStatus --output text)" = "True" ] && break
    sleep 10
  done
  aws sesv2 get-email-identity --email-identity "$D" --query '{verified: VerifiedForSendingStatus, dkim: DkimAttributes.Status}'

  # The spike Worker in workerd, with the role's temporary credentials.
  (cd "$HERE" && worker-build --release --locked >/dev/null)
  umask 077
  printf 'AWS_ACCESS_KEY_ID=%s\nAWS_SECRET_ACCESS_KEY=%s\nAWS_SESSION_TOKEN=%s\nAWS_REGION=%s\n' \
    "$AWS_ACCESS_KEY_ID" "$AWS_SECRET_ACCESS_KEY" "$AWS_SESSION_TOKEN" "$R" > "$HERE/.dev.vars"
  (cd "$HERE" && "$WRANGLER" dev --local --port 8787 > "$HERE/dev.log" 2>&1 &)
  for _ in $(seq 1 60); do curl -s -o /dev/null localhost:8787 && break; sleep 2; done
  rm -f "$HERE/.dev.vars"
  w() { curl -sS -X POST -H 'content-type: application/json' --data-binary "$2" "localhost:8787$1"; }

  local started small big retired
  started=$(date -u +%s)
  small=$(w /s8/send "{\"from\":\"s8@$D\",\"to\":[\"user+tag@$D\"],\"subject\":\"S8 small\"}")
  big=$(w /s8/send "{\"from\":\"s8@$D\",\"to\":[\"big@$D\"],\"subject\":\"S11 39 MB\",\"pad_bytes\":39000000}")
  retired=$(w /s8/send "{\"from\":\"s8@$D\",\"to\":[\"retired@$D\"],\"subject\":\"S11 retired\"}")
  echo "send small:   $(echo "$small" | jq -c 'del(.response.MessageId) + {message_id: .response.MessageId}')"
  echo "send 39 MB:   $(echo "$big" | jq -c '{status, error_type, raw_len, response}')"
  echo "send retired: $(echo "$retired" | jq -c '{status, error_type, response}')"

  # The backstop: read every notification from SQS through the Worker until the three have arrived.
  : > "$HERE/envelopes.jsonl"
  for _ in $(seq 1 40); do
    w /s11/receive "{\"queue_url\":\"$(queue_url)\",\"delete\":true}" | jq -c '.bodies[]' >> "$HERE/envelopes.jsonl" || true
    local subjects
    subjects=$(jq -r 'fromjson | .Message | fromjson | .mail.commonHeaders.subject // ""' "$HERE/envelopes.jsonl" 2>/dev/null | sort -u)
    if grep -q "S8 small" <<<"$subjects" && grep -q "S11 39 MB" <<<"$subjects" && [ "$(wc -l < "$HERE/envelopes.jsonl")" -ge 3 ]; then break; fi
    sleep 10
  done

  echo "== notifications ($(wc -l < "$HERE/envelopes.jsonl"), $(( $(date -u +%s) - started )) s after the first send)"
  : > "$OUT"
  while read -r envelope; do
    local message key bucket verdict object
    message=$(echo "$envelope" | jq -r 'fromjson | .Message')
    key=$(echo "$message" | jq -r '.receipt.action.objectKey // empty')
    bucket=$(echo "$message" | jq -r '.receipt.action.bucketName // empty')
    verdict=$(w /s8/verify "$(jq -nc --argjson e "$envelope" --arg t "$(topic_arn)" '{envelope: $e, topic_arn: $t, max_age_seconds: 3600}')")
    object='null'
    [ -n "$key" ] && object=$(w /s11/get "$(jq -nc --arg b "$bucket" --arg k "$key" '{bucket: $b, key: $k}')")
    jq -nc --argjson m "$message" --argjson v "$verdict" --argjson o "$object" '{
      notification: { type: $m.notificationType, recipients: $m.receipt.recipients, action: $m.receipt.action,
                      subject: $m.mail.commonHeaders.subject, from: $m.mail.commonHeaders.from,
                      spam: $m.receipt.spamVerdict.status, virus: $m.receipt.virusVerdict.status,
                      spf: $m.receipt.spfVerdict.status, dkim: $m.receipt.dkimVerdict.status, dmarc: $m.receipt.dmarcVerdict.status,
                      message_keys: ($m | keys), receipt_keys: ($m.receipt | keys) },
      verify: $v, object: $o }' | tee -a "$OUT"
  done < "$HERE/envelopes.jsonl"
  echo "== workerd log tail"; tail -5 "$HERE/dev.log" || true
}

case "$PHASE" in
  setup) setup ;;
  exercise) exercise ;;
  cleanup) cleanup ;;
  *) echo "unknown phase $PHASE" >&2; exit 2 ;;
esac
