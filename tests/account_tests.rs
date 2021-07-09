mod common;

#[cfg(test)]
mod tests {
    use actix_web::test;
    use mockito::{Matcher, mock};
    use serde_json::{Value, json};
    use assert_json_diff::assert_json_eq;
    use crate::common::{freeze_time, http::{get, post, put}, new_uuid, rabbit::listen_to_topic, run_test, start_app};

    // TODO: Mock to match on correlation-id, test response and rabbit have same id.

    #[actix_rt::test]
    async fn test_create_account_happy_path() {
        run_test(async {
            // Given the environment is set-up.
            let mut service = test::init_service(start_app().await).await;
            let rabbit = listen_to_topic("account.created").await;
            let auth_mock = mock_auth_ok();
            let account_id = new_uuid();
            freeze_time(&mut service, "2021-07-03T04:52:49.830Z").await;

            // When a request is made to create an account.
            let mut resp = post("/create-account")
                .header("content-type", "application/json")
                .query_param("ignore", "me") // Just here to remove unused on .query_param :)
                .body(json!({
                    "accountId": account_id,
                    "salutation": "Mr Blobby",
                    "profileId": "DEFAULT"
                }))
                .send(&mut service)
                .await;

            // Then the response looks correct.
            assert_eq!(resp.status(), 201);
            let actual: Value = resp.read_body().await;
            let expected = json!({
                    "accountId": account_id,
                    "profileId": "DEFAULT",
                    "status": "ACTIVE",
                    "salutation": "Mr Blobby",
                    "created": "2021-07-03T04:52:49.830Z"
                });
            assert_json_eq!(actual, expected.clone());

            // And the auth service was called.
            auth_mock.assert();

            // And we can retrieve the created account.
            let mut resp = get(&format!("/account/{}", account_id))
                .send(&mut service)
                .await;

            assert_eq!(resp.status(), 200);
            let actual: Value = resp.read_body().await;
            assert_json_eq!(actual, expected.clone());

             // And a RabbitMQ notification was generated.
             rabbit.assert_payload_received(expected.clone()).await;
        }).await;
    }

    #[actix_rt::test]
    async fn test_update_account_status_happy_path() {
        run_test(async {
            // Given the environment is set-up.
            let mut service = test::init_service(start_app().await).await;
            let rabbit = listen_to_topic("account.status.updated").await;
            let auth_mock = mock_auth_ok();
            let account_id = new_uuid();
            freeze_time(&mut service, "2021-07-04T04:52:49.830Z").await;

            // And an account already exists.
            let resp = post("/create-account")
                .header("content-type", "application/json")
                .body(json!({
                    "accountId": account_id,
                    "salutation": "Mr Blobby",
                    "profileId": "DEFAULT"
                }))
                .send(&mut service)
                .await;
            assert_eq!(resp.status(), 201);
            auth_mock.assert();

            // When the status is updated.
            let resp = put("/update-account-status")
                .header("content-type", "application/json")
                .body(json!({
                    "accountId": account_id,
                    "status": "RESTRICTED"
                }))
                .send(&mut service)
                .await;

            // Then the response is successful.
            assert_eq!(resp.status(), 200);

            // And a RabbitMQ notification was generated.
            rabbit.assert_payload_received(json!({
                "accountId": account_id,
                "oldStatus": "ACTIVE",
                "newStatus": "RESTRICTED"
            })).await;
        }).await;
    }

    #[actix_rt::test]
    async fn test_ensure_default_account_profile_exists() {
        run_test(async {
            // Given the environment is set-up.
            let mut service = test::init_service(start_app().await).await;

            // When a request to retrieve the DEFAULT account profile is made.
            let mut resp = get("/account-profile/DEFAULT")
                .send(&mut service)
                .await;

            // Then the response looks correct.
            assert_eq!(resp.status(), 200);
            let actual: Value = resp.read_body().await;
            let expected = json!({
                "profileId": "DEFAULT"
            });
            assert_json_eq!(actual, expected.clone());
        }).await;
    }

    #[actix_rt::test]
    async fn test_ensure_default_device_profile_exists() {
        run_test(async {
            // Given the environment is set-up.
            let mut service = test::init_service(start_app().await).await;

            // When a request to retrieve the DEFAULT device profile is made.
            let mut resp = get("/device-profile/DEFAULT")
                .send(&mut service)
                .await;

            // Then the response looks correct.
            assert_eq!(resp.status(), 200);
            let actual: Value = resp.read_body().await;
            let expected = json!({
                "profileId": "DEFAULT"
            });
            assert_json_eq!(actual, expected.clone());
        }).await;
    }

    //
    // Create a mock auth service response. This is just an example downstream service our service
    // may call.
    //
    fn mock_auth_ok() -> mockito::Mock {
        mock("POST", "/auth/get-claims")
            .match_query(Matcher::UrlEncoded("param1".into(), "value1".into()))
            .match_header("x-correlation-id", Matcher::Any)
            .match_header("user-agent", "Nails")
            .with_header("content-type", "application/json")
            .with_status(200)
            .with_body(r#"
            {
                "claims": [
                    "create-account",
                    "read-own-account",
                    "etc"
                ]
            }"#)
            .create()
    }
}
