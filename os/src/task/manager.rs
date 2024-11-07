//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::config::BIGSTRIDE;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    // ready_queue: VecDeque<Arc<TaskControlBlock>>,
    ready_queue: Vec<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // self.ready_queue.pop_front()

        // find the TCB with minist stride in ready_queue and it's status must be Ready
        let mut min_stride = isize::MAX;
        let mut min_stride_index = 0;
        for (index, tcb) in self.ready_queue.iter().enumerate() {
            let stride = tcb.inner_exclusive_access().stride;
            if stride < min_stride {
                min_stride = stride;
                min_stride_index = index;
            }
        }
        if min_stride == isize::MAX {
            return None;
        } else {
            let tcb = self.ready_queue.remove(min_stride_index);
            // for the tcb fetched, we need to update its stride
            let mut inner = tcb.inner_exclusive_access();
            inner.stride += BIGSTRIDE / inner.priority;
            drop(inner); // release tcb
            return Some(tcb);
        }
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
