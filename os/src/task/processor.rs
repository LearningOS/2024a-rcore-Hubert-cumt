//!Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{TaskContext, TaskControlBlock};
use crate::config::MAX_SYSCALL_NUM;
use crate::mm::{translate_va_to_pa, MapPermission, PhysAddr, VirtAddr, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::timer::get_time_ms;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

/// Processor management structure
pub struct Processor {
    ///The task currently executing on the current processor
    current: Option<Arc<TaskControlBlock>>,

    ///The basic control flow of each core, helping to select and switch process
    idle_task_cx: TaskContext,
}

impl Processor {
    ///Create an empty Processor
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    ///Get mutable reference to `idle_task_cx`
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    ///Get current task in moving semanteme
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    ///Get current task in cloning semanteme
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }

    /// translate virtual address to physical address
    fn trans_vir2phy(&self, vir_addr: VirtAddr) -> Option<PhysAddr> {
        let token = self.current().unwrap().inner_exclusive_access().memory_set.token();
        translate_va_to_pa(token, vir_addr)
    }

    /// count the syscall times of current task
    fn count_syscall(&mut self, syscall_id: usize) {
        let curr = self.current().unwrap();
        let mut inner = curr.inner_exclusive_access();
        inner.syscall_times[syscall_id] += 1;
    }

    /// get the syscall times of current task
    fn get_sycall_times(&self) -> [u32; MAX_SYSCALL_NUM] {
        self.current().unwrap().inner_exclusive_access().syscall_times
    }

    /// get the first time called of current task
    fn get_first_time_called(&self) -> usize {
        self.current().unwrap().inner_exclusive_access().time_first_called
    }

    /// map a new memory
    fn  do_memory_map(&mut self, start: VirtAddr, end: VirtAddr, port: usize) -> isize {
        let curr = self.current().unwrap();
        let mut inner = curr.inner_exclusive_access();
        
        // Transfer port to permission
        let mut permission = 0 as u8;
        // Readable
        if port & 0x1 != 0 {
            permission |= MapPermission::R.bits();
        }
        // Writable
        if port & 0x2 != 0 {
            permission |= MapPermission::W.bits();
        }
        // Executable
        if port & 0x4 != 0 {
            permission |= MapPermission::X.bits();
        }
        // User accessible : PTE_U
        permission |= MapPermission::U.bits();

         
        let permission = MapPermission::from_bits(permission).unwrap();

        if inner.memory_set.insert_framed_area(start, end, permission) == -1 {
            return -1;
        }
        
        0
    }

    /// unmap a memory
    fn do_memory_unmap(&mut self, vst: VirtPageNum, ved: VirtPageNum) -> isize {
        let curr = self.current().unwrap();
        let mut inner = curr.inner_exclusive_access();

        if inner.memory_set.remove_by_given(vst, ved) == -1 {
            return -1;
        }
        
        0
    }

    /// set the priority of current task
    fn set_prio(&mut self, prio: isize) -> isize {
        let curr = self.current().unwrap();
        let mut inner = curr.inner_exclusive_access();
        inner.priority = prio;
        inner.priority
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

///The main part of process execution and scheduling
///Loop `fetch_task` to get the process that needs to run, and switch the process through `__switch`
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            if task_inner.time_first_called == 0 {
                task_inner.time_first_called = get_time_ms();
            }
            task_inner.task_status = TaskStatus::Running;
            // release coming task_inner manually
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            // release processor manually
            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
        }
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// Get the current user token(addr of page table)
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

///Get the mutable reference to trap context of current task
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

///Return to idle control flow for new scheduling
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}

/// Transfer virtual address to physical address
pub fn translate_address(vir_addr: VirtAddr) -> Option<PhysAddr> {
    PROCESSOR.exclusive_access().trans_vir2phy(vir_addr)
}

/// count the syscall
pub fn count_syscall(syscall_id: usize) {
    PROCESSOR.exclusive_access().count_syscall(syscall_id);
}

/// get the syscall times
pub fn get_syscall_times() -> [u32; MAX_SYSCALL_NUM] {
    PROCESSOR.exclusive_access().get_sycall_times()
}

/// get the first time called
pub fn get_first_time_called() -> usize {
    PROCESSOR.exclusive_access().get_first_time_called()
}

/// map a new memory
pub fn do_memory_map(start: VirtAddr, end: VirtAddr, port: usize) -> isize {
    PROCESSOR.exclusive_access().do_memory_map(start, end, port)
}

/// unmap a memory
pub fn do_memory_unmap(start: VirtPageNum, end: VirtPageNum) -> isize {
    PROCESSOR.exclusive_access().do_memory_unmap(start, end)
}

/// set the priority of current task
pub fn set_prio(prio: isize) -> isize {
    PROCESSOR.exclusive_access().set_prio(prio)
}
