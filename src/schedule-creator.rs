use aws_config::default_provider::credentials::DefaultCredentialsChain;
use aws_config::meta::region::RegionProviderChain;
use aws_sig_auth::signer::{OperationSigningConfig, RequestConfig, SigV4Signer};
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::byte_stream::ByteStream;
use aws_types::credentials::ProvideCredentials;
use aws_types::region::{Region, SigningRegion};
use aws_types::SigningService;
use http::Request;
use lambda_http::service_fn;
use lambda_runtime::{Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::time::SystemTime;

#[tokio::main]
async fn main() -> Result<(), Error> {
    lambda_runtime::run(service_fn(handler)).await?;

    return Ok(());
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct SchedulerTimeWindow {
    mode: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct SchedulerRequest {
    client_token: String,
    schedule_expression: String,
    schedule_expression_timezone: String,
    name: String,
    time_window: SchedulerTimeWindow,
}

async fn handler(event: LambdaEvent<SchedulerRequest>) -> Result<Value, Error> {
    println!("Request payload: {:?}", event.payload);

    let name = event.payload.name;
    let current_region = RegionProviderChain::default_provider()
        .region()
        .await
        .expect("region should be present");

    let uri = format!(
        "https://scheduler.{}.amazonaws.com/schedules/{}",
        current_region.to_string(),
        name
    );
    let client_token = event.payload.client_token;
    let schedule_expression = event.payload.schedule_expression;
    let schedule_expression_timezone = event.payload.schedule_expression_timezone;

    let target_arn = env::var("SCHEDULER_TARGET_ARN").expect("SCHEDULER_TARGET_ARN must be set");
    let target_role = env::var("SCHEDULER_ROLE_ARN").expect("SCHEDULER_ROLE_ARNust be set");
    let target_dlq_arn = env::var("SCHEDULER_DLQ_ARN").expect("SCHEDULER_DLQ_ARN must be set");

    let body = json!({
        "ClientToken": client_token,
        "ScheduleExpression": schedule_expression,
        "ScheduleExpressionTimezone": schedule_expression_timezone,
        "FlexibleTimeWindow": {
            "Mode": "OFF"
        },
        "RetryPolicy": {
            "MaximumRetryAttempts": 0,
            "MaximumEventAgeInSeconds": 60
        },
        "Target": {
            "Arn": target_arn,
            "RoleArn": target_role,
            "Input": "hi!",
            "DeadLetterConfig": {
                "Arn": target_dlq_arn
            }
        }
    })
    .to_string();
    let sdk_body = SdkBody::from(body);
    let mut sdk_request = Request::builder()
        .method("POST")
        .uri(uri) // this one was difficult to compute
        .body(sdk_body)
        .expect("request is valid");

    let credentials_provider = DefaultCredentialsChain::builder()
        .region(current_region.clone())
        .build()
        .await;

    sign_request(&mut sdk_request, current_region, &credentials_provider).await?;

    let client = reqwest::Client::new();
    let req = convert_req(&client, sdk_request);
    let res = client.execute(req).await?;

    let status = res.status();
    match status.as_u16() {
        200..=299 => println!("Request scheduled! {}", res.text().await?),
        _ => panic!(
            "Request failed with status:{}, body: {}",
            status,
            res.text().await?
        ),
    }

    return Ok(json!({"pk":name, "status": "SCHEDULED"}));
}

async fn sign_request(
    mut request: &mut http::Request<SdkBody>,
    region: Region,
    credentials_provider: &impl ProvideCredentials,
) -> Result<(), Error> {
    let now = SystemTime::now();
    let signer = SigV4Signer::new();
    let request_config = RequestConfig {
        request_ts: now,
        region: &SigningRegion::from(region),
        service: &SigningService::from_static("scheduler"),
        payload_override: None,
    };
    signer.sign(
        &OperationSigningConfig::default_config(),
        &request_config,
        &credentials_provider.provide_credentials().await?,
        &mut request,
    )?;
    Ok(())
}

fn convert_req(client: &reqwest::Client, req: http::Request<SdkBody>) -> reqwest::Request {
    let (head, body) = req.into_parts();
    let url = head.uri.to_string();

    let body = {
        // `SdkBody` doesn't currently impl stream but we can wrap
        // it in a `ByteStream` and then we're good to go.
        let stream = ByteStream::new(body);
        // Requires `reqwest` crate feature "stream"
        reqwest::Body::wrap_stream(stream)
    };

    client
        .request(head.method, url)
        .headers(head.headers)
        .version(head.version)
        .body(body)
        .build()
        .expect("request is valid")
}
