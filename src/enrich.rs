use aws_lambda_events::dynamodb::EventRecord;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use serde_dynamo::from_item;
use serde_json::{json, Value};

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("before runtime invoke");

    lambda_runtime::run(service_fn(handler)).await?;

    println!("after runtime invoke");

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

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SchedulerItem {
    pk: String,
    date: String,
    status: String,
}

async fn handler(event: LambdaEvent<Vec<EventRecord>>) -> Result<Value, Error> {
    println!("Request payload: {:?}", event.payload);

    let stream_entry = event.payload.get(0).unwrap().change.new_image.clone();
    let scheduler_item: SchedulerItem = from_item(stream_entry).unwrap();

    let client_token = scheduler_item.pk.clone();
    let name = scheduler_item.pk;

    let scheduler_request = SchedulerRequest {
        client_token: client_token,
        name,
        schedule_expression: format!("at({})", scheduler_item.date),
        schedule_expression_timezone: "UTC".to_string(),
        time_window: SchedulerTimeWindow {
            mode: "OFF".to_string(),
        },
    };

    println!("Scheduler request: {:?}", scheduler_request);
    return Ok(json!(scheduler_request));
}
