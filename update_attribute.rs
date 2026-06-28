use aws_sdk_dynamodb::{
    types::AttributeValue,
    Client,
};
use std::collections::HashMap;

pub async fn conditional_update(client: &Client, table_name: &str) -> Result<(), aws_sdk_dynamodb::Error> {
    let mut key = HashMap::new();
    key.insert("UserId".to_string(), AttributeValue::S("user_123".to_string()));

    // Define the condition (e.g., only update if the user's status is 'Active')
    let condition_expression = "attribute_exists(#status) AND #status = :active_status";
    
    // Execute the update
    let request = client
        .update_item()
        .table_name(table_name)
        .set_key(Some(key))
        .update_expression("SET #status = :new_status")
        .condition_expression(condition_expression)
        .expression_attribute_names("#status", "Status")
        .expression_attribute_values(":active_status", AttributeValue::S("Active".to_string()))
        .expression_attribute_values(":new_status", AttributeValue::S("Suspended".to_string()));

    request.send().await?;
    Ok(())
}

