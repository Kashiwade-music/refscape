use refscape_model::{
    CancellationToken, ErrorKind, JobId, OperationContext, ProjectEpoch, RefscapeError,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobClass {
    Switch,
    Edit,
    Arrange,
    Query,
    Search,
    Hover,
    Inspection,
    Save,
}
pub(crate) struct JobRecord {
    pub class: JobClass,
    pub context: OperationContext,
}
#[derive(Default)]
pub(crate) struct JobRegistry {
    next: u64,
    pub records: HashMap<JobId, JobRecord>,
    pub policy: OperationBudgetPolicy,
    pub catalog_files: usize,
}

/// A single absolute budget includes individual RPC upper bounds, retries, metadata
/// processes and CPU/I/O margin. It is computed once, before the first request.
#[derive(Clone, Debug)]
pub struct OperationBudgetPolicy {
    rpc_timeout: Duration,
    rpc_attempts: u32,
    metadata_timeout: Duration,
    processing_margin: Duration,
}
impl Default for OperationBudgetPolicy {
    fn default() -> Self {
        Self {
            rpc_timeout: Duration::from_secs(120),
            rpc_attempts: 3,
            metadata_timeout: Duration::from_secs(45),
            processing_margin: Duration::from_secs(60),
        }
    }
}
impl OperationBudgetPolicy {
    pub fn new(
        rpc_timeout: Duration,
        rpc_attempts: u32,
        metadata_timeout: Duration,
        processing_margin: Duration,
    ) -> Result<Self, RefscapeError> {
        if rpc_attempts == 0 || rpc_timeout.is_zero() {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "Operation budgets require a positive RPC limit and attempt count",
            ));
        }
        let policy = Self {
            rpc_timeout,
            rpc_attempts,
            metadata_timeout,
            processing_margin,
        };
        policy.for_requests(4, true)?;
        Ok(policy)
    }
    pub fn for_requests(&self, requests: usize, metadata: bool) -> Result<Duration, RefscapeError> {
        let invalid = || RefscapeError::new(ErrorKind::InvalidData, "Operation budget overflow");
        let requests = u32::try_from(requests).map_err(|_| invalid())?;
        let attempts = requests
            .checked_mul(self.rpc_attempts)
            .ok_or_else(invalid)?;
        let budget = self
            .rpc_timeout
            .checked_mul(attempts)
            .and_then(|duration| {
                duration.checked_add(if metadata {
                    self.metadata_timeout
                } else {
                    Duration::ZERO
                })
            })
            .and_then(|duration| duration.checked_add(self.processing_margin))
            .ok_or_else(invalid)?;
        Instant::now().checked_add(budget).ok_or_else(invalid)?;
        Ok(budget)
    }
    fn for_operation(&self, class: JobClass, files: usize) -> Result<Duration, RefscapeError> {
        let requests = match class {
            JobClass::Switch => 4, // initialize, readiness, seed symbols, optional token preparation
            JobClass::Search => files.max(1).saturating_mul(2).saturating_add(1),
            JobClass::Edit => files.max(1).saturating_mul(3).saturating_add(1),
            JobClass::Query | JobClass::Inspection => 2,
            JobClass::Hover => 1,
            JobClass::Arrange | JobClass::Save => 0,
        };
        self.for_requests(
            requests,
            matches!(
                class,
                JobClass::Switch | JobClass::Search | JobClass::Edit | JobClass::Query
            ),
        )
    }
}
impl JobRegistry {
    pub fn begin(&mut self, class: JobClass, epoch: ProjectEpoch) -> OperationContext {
        self.next += 1;
        // Overflow expires immediately and is returned as a typed timeout by the
        // worker; neither arithmetic nor an extreme catalog can panic the owner.
        let budget = self
            .policy
            .for_operation(class, self.catalog_files)
            .unwrap_or_default();
        let now = Instant::now();
        let context = OperationContext::new(
            JobId(self.next),
            epoch,
            now.checked_add(budget).unwrap_or(now),
        );
        self.records.insert(
            context.id,
            JobRecord {
                class,
                context: context.clone(),
            },
        );
        context
    }
    pub fn cancel_class(&mut self, class: JobClass) -> Vec<(JobId, CancellationToken)> {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter(|(_, record)| record.class == class)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| {
                self.records.remove(&id).map(|record| {
                    record.context.cancel.cancel();
                    (id, record.context.cancel)
                })
            })
            .collect()
    }
    pub fn busy(&self) -> bool {
        self.records
            .values()
            .any(|record| !matches!(record.class, JobClass::Hover | JobClass::Save))
    }
}
