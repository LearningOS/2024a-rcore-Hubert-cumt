//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;


use crate::config::MAX_SYSCALL_NUM;
use crate::loader::{get_app_data, get_num_app};
use crate::sync::UPSafeCell;
use crate::timer::get_time_ms;
use crate::trap::TrapContext;
use crate::mm::{MapPermission, PhysPageNum, VirtAddr, VirtPageNum};
use alloc::vec::Vec;
use lazy_static::*;
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;

            // record the first running time
            if inner.tasks[next].first_running_time == 0 {
                inner.tasks[next].first_running_time = get_time_ms();
            }

            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    /// Translate vpn to ppn    
    fn trans_vpn2ppn(&self, vpn: VirtPageNum) -> PhysPageNum {
        let inner = self.inner.exclusive_access();
        let ppn = inner.tasks[inner.current_task].memory_set.translate(vpn).unwrap().ppn();
        ppn
    }

    /// Translate virtual address to physical address
    fn trans_vir2phy(&self, vir_addr: usize) -> usize {
        let va = VirtAddr(vir_addr);
        // Get the vpn and offset of the virtual address
        let offset = va.page_offset();

        // align the virtual address to get the vpn
        let vpn = va.floor();
        // let phy_pte = inner.tasks[inner.current_task].memory_set.translate(vpn).unwrap();
        // let ppn = phy_pte.ppn();
        
        let ppn = trans_vpn2ppn(vpn);
        let phy_addr = (ppn.0 << 12) + offset;
        phy_addr
    }

    /// Count syscall times of current task
    fn count_syscall(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].syscall_times[syscall_id] += 1;
    }

    /// get the current 'Running' task's syscall times
    fn get_syscall_times(&self) -> [u32; MAX_SYSCALL_NUM] {
        let inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].syscall_times
    }

    /// Get this task's first running time
    fn get_first_running_time(&self) -> usize {
        let inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].first_running_time
    }

    /// mmap the memory
    fn do_memory_map(&self, start: VirtAddr , end: VirtAddr, port: usize) -> isize {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;

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

        // check memory range has been mapped
        // if inner.tasks[cur].memory_set.check_mapped(start, end) {
        //     return -1;
        // }

        // println!("port: {:#b}, permission: {:#b}", port, permission);

        let permission = MapPermission::from_bits(permission).unwrap();

        if inner.tasks[cur].memory_set.insert_framed_area(start, end, permission) == -1 {
            return -1;
        }

        0
    }

    /// unmap the memory
    fn do_memory_unmap(&self, start: VirtAddr, end: VirtAddr) -> isize {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;

        if inner.tasks[cur].memory_set.remove_by_given(start, end) == -1 {
            return -1;
        }

        0
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// Translate vpn to ppn
pub fn trans_vpn2ppn(vpn: VirtPageNum) -> PhysPageNum {
    TASK_MANAGER.trans_vpn2ppn(vpn)
}

/// Translate virtual address to physical
pub fn trans_vir2phy(vir_addr: usize) -> usize {
    TASK_MANAGER.trans_vir2phy(vir_addr)
}

/// Count syscall times of current task
pub fn count_syscall(syscall_id: usize) {
    TASK_MANAGER.count_syscall(syscall_id);
}

/// Get the current 'Running' task's syscall times
pub fn get_syscall_times() -> [u32; MAX_SYSCALL_NUM] {
    TASK_MANAGER.get_syscall_times()
}

/// Get this task's first running time
pub fn get_first_running_time() -> usize {
    TASK_MANAGER.get_first_running_time()
}

/// mmap the memory
pub fn do_memory_map(start: VirtAddr , end: VirtAddr, port: usize) -> isize {
    TASK_MANAGER.do_memory_map(start, end, port)
}

/// unmapp the memory
pub fn do_memory_unmap(start: VirtAddr, end: VirtAddr) -> isize {
    TASK_MANAGER.do_memory_unmap(start, end)
}
