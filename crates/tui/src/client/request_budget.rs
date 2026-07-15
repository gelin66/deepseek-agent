use std::fmt;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug)]
pub(crate) struct SharedApiRequestBudget {
    state: Arc<Mutex<ApiRequestBudgetState>>,
}

#[derive(Debug)]
struct ApiRequestBudgetState {
    limit: NonZeroU32,
    started: u32,
    denied: u32,
    sealed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApiRequestBudgetSnapshot {
    pub(crate) limit: u32,
    pub(crate) started: u32,
    pub(crate) denied: u32,
    pub(crate) sealed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApiRequestBudgetError {
    Exhausted { limit: u32, started: u32 },
    Sealed { limit: u32, started: u32 },
}

impl fmt::Display for ApiRequestBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted { limit, started } => {
                write!(
                    formatter,
                    "DeepSeek API 请求预算已用尽（已发起：{started}，上限：{limit}）"
                )
            }
            Self::Sealed { limit, started } => write!(
                formatter,
                "DeepSeek API 请求预算已封存，不能再发起请求（已发起：{started}，上限：{limit}）"
            ),
        }
    }
}

impl std::error::Error for ApiRequestBudgetError {}

impl SharedApiRequestBudget {
    pub(crate) fn new(limit: NonZeroU32) -> Self {
        Self {
            state: Arc::new(Mutex::new(ApiRequestBudgetState {
                limit,
                started: 0,
                denied: 0,
                sealed: false,
            })),
        }
    }

    pub(crate) fn try_reserve(&self) -> Result<ApiRequestBudgetSnapshot, ApiRequestBudgetError> {
        let mut state = self.lock_state();

        if state.sealed {
            state.denied = state.denied.saturating_add(1);
            return Err(ApiRequestBudgetError::Sealed {
                limit: state.limit.get(),
                started: state.started,
            });
        }

        if state.started >= state.limit.get() {
            state.denied = state.denied.saturating_add(1);
            return Err(ApiRequestBudgetError::Exhausted {
                limit: state.limit.get(),
                started: state.started,
            });
        }

        state.started += 1;
        Ok(snapshot_of(&state))
    }

    pub(crate) fn seal_and_snapshot(&self) -> ApiRequestBudgetSnapshot {
        let mut state = self.lock_state();
        state.sealed = true;
        snapshot_of(&state)
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> ApiRequestBudgetSnapshot {
        snapshot_of(&self.lock_state())
    }

    fn lock_state(&self) -> MutexGuard<'_, ApiRequestBudgetState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

fn snapshot_of(state: &ApiRequestBudgetState) -> ApiRequestBudgetSnapshot {
    ApiRequestBudgetSnapshot {
        limit: state.limit.get(),
        started: state.started,
        denied: state.denied,
        sealed: state.sealed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn concurrent_reservations_never_exceed_limit() {
        const CALLERS: usize = 32;
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        let barrier = Arc::new(Barrier::new(CALLERS + 1));
        let handles = (0..CALLERS)
            .map(|_| {
                let budget = budget.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    budget.try_reserve()
                })
            })
            .collect::<Vec<_>>();

        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 3);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    **result
                        == Err(ApiRequestBudgetError::Exhausted {
                            limit: 3,
                            started: 3,
                        })
                })
                .count(),
            CALLERS - 3
        );
        assert_eq!(
            budget.snapshot(),
            ApiRequestBudgetSnapshot {
                limit: 3,
                started: 3,
                denied: (CALLERS - 3) as u32,
                sealed: false,
            }
        );
    }

    #[test]
    fn sealed_budget_rejects_new_reservations() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());
        budget.try_reserve().unwrap();

        assert_eq!(
            budget.seal_and_snapshot(),
            ApiRequestBudgetSnapshot {
                limit: 3,
                started: 1,
                denied: 0,
                sealed: true,
            }
        );
        assert_eq!(
            budget.try_reserve(),
            Err(ApiRequestBudgetError::Sealed {
                limit: 3,
                started: 1,
            })
        );
        assert_eq!(budget.snapshot().denied, 1);
    }

    #[test]
    fn using_last_slot_succeeds_before_exhaustion_is_observed() {
        let budget = SharedApiRequestBudget::new(NonZeroU32::new(3).unwrap());

        budget.try_reserve().unwrap();
        budget.try_reserve().unwrap();
        let final_reservation = budget.try_reserve().unwrap();

        assert_eq!(final_reservation.started, final_reservation.limit);
        assert_eq!(final_reservation.denied, 0);
        assert_eq!(
            budget.try_reserve(),
            Err(ApiRequestBudgetError::Exhausted {
                limit: 3,
                started: 3,
            })
        );
        assert_eq!(budget.snapshot().denied, 1);
    }
}
