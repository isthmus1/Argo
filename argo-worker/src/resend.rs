//! Delivering a fenced write whose outcome may be lost (timed out, dropped). A lost outcome is
//! not a rejection: the write may not have committed, so giving up on it would throw away a
//! good result. Resending is safe because every write quotes the claim's generation: once one
//! copy commits, the claim is no longer `Claimed` and the module rejects the rest.

use crate::db::CallError;

/// Sends before an outcome that stays unknown is given up on.
pub const SENDS: u32 = 3;

/// `send` until it succeeds, is rejected, or [`SENDS`] outcomes were lost. After any lost
/// outcome, `landed` says whether an earlier copy committed after all (the subscription shows
/// it), which turns a later rejection, or a final loss, into success.
pub async fn deliver(
    mut send: impl AsyncFnMut() -> Result<(), CallError>,
    mut landed: impl AsyncFnMut() -> bool,
) -> Result<(), CallError> {
    let (mut sent, mut lost) = (0, false);
    loop {
        sent += 1;
        let Err(e) = send().await else {
            return Ok(());
        };
        lost |= matches!(e, CallError::Unknown(_));
        if lost && landed().await {
            return Ok(());
        }
        if matches!(e, CallError::Rejected(_)) || sent >= SENDS {
            return Err(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::{SENDS, deliver};
    use crate::db::CallError::{self, Rejected, Unknown};

    /// Deliver against scripted outcomes; the result and how many sends it took.
    async fn run(outcomes: &[Result<(), CallError>], landed: bool) -> (Result<(), CallError>, u32) {
        let script = RefCell::new(outcomes.iter().cloned().collect::<VecDeque<_>>());
        let sends = RefCell::new(0);
        let send = async || {
            *sends.borrow_mut() += 1;
            script
                .borrow_mut()
                .pop_front()
                .expect("more sends than scripted")
        };
        let result = deliver(send, async || landed).await;
        (result, sends.into_inner())
    }

    fn lost() -> Result<(), CallError> {
        Err(Unknown("no reducer outcome".into()))
    }

    fn rejected() -> Result<(), CallError> {
        Err(Rejected("chunk is Done, not Claimed".into()))
    }

    #[tokio::test]
    async fn a_lost_outcome_that_did_not_commit_is_resent() {
        assert_eq!(run(&[lost(), Ok(())], false).await, (Ok(()), 2));
    }

    #[tokio::test]
    async fn a_lost_outcome_that_committed_is_success() {
        assert_eq!(run(&[lost()], true).await, (Ok(()), 1));
    }

    #[tokio::test]
    async fn a_resend_rejected_because_the_first_landed_is_success() {
        let script = RefCell::new(VecDeque::from([lost(), rejected()]));
        let landed = RefCell::new(VecDeque::from([false, true]));
        let send = async || script.borrow_mut().pop_front().unwrap();
        let result = deliver(send, async || landed.borrow_mut().pop_front().unwrap()).await;
        assert_eq!(result, Ok(()));
    }

    #[tokio::test]
    async fn a_rejection_is_final_and_never_resent() {
        assert_eq!(run(&[rejected()], true).await, (rejected(), 1));
    }

    #[tokio::test]
    async fn outcomes_that_stay_lost_give_up_after_the_budget() {
        let all_lost = vec![lost(); SENDS as usize];
        assert_eq!(run(&all_lost, false).await, (lost(), SENDS));
    }
}
