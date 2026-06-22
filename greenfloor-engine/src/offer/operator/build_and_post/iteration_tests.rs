use std::time::Instant;

use super::create_offer_for_post;
use super::PostIterationOutcome;
use crate::offer::codec::verify_offer_for_dexie;
use crate::offer::operator::build_and_post::context::sample_resolved_build_and_post_context;
use crate::test_support::build_and_post::unused_post_iteration_request;

#[tokio::test]
async fn create_offer_for_post_rejects_unverifiable_offer_text() {
    let mut ctx = sample_resolved_build_and_post_context();
    ctx.test_overrides.offer_text = Some("not-an-offer".to_string());
    let offer_text = "not-an-offer";
    let request = unused_post_iteration_request(false, Some(offer_text));
    let expected_verify_error = verify_offer_for_dexie(offer_text).expect("verify error");

    let outcome = create_offer_for_post(&request, &ctx, Instant::now())
        .await
        .expect("iteration result")
        .expect_err("verify failure");

    match outcome {
        PostIterationOutcome::Failure(failure) => {
            assert_eq!(failure.error, expected_verify_error);
        }
        _ => panic!("expected verify failure"),
    }
}
