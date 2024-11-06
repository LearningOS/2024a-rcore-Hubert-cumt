//! Process management syscalls
use crate::{
    config::MAX_SYSCALL_NUM, mm::{check_rest_memory, VirtAddr}, task::{
        change_program_brk, do_memory_map, do_memory_unmap, exit_current_and_run_next, get_first_running_time, get_syscall_times, suspend_current_and_run_next, trans_vir2phy, TaskStatus
    }, timer::{get_time_ms, get_time_us}
    
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");

    let sec = get_time_ms() / 1000;
    let usec = get_time_us();

    let va = VirtAddr(ts as usize);
    let phy_address = trans_vir2phy(va.0) as *mut TimeVal;

    unsafe {
        *phy_address = TimeVal {
            sec: sec,
            usec: usec,
        };
    }
    
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    
    let status = TaskStatus::Running;
    let syscall_times = get_syscall_times();
    let time = get_time_ms() - get_first_running_time();
    
    let va = VirtAddr(ti as usize);
    let pa = trans_vir2phy(va.0) as *mut TaskInfo;

    unsafe {
        *pa = TaskInfo {
            status: status,
            syscall_times: syscall_times,
            time: time,
        };
    }

    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap");
    let vst = VirtAddr(start);

    // check the start address is valid
    if vst.page_offset() != 0 {
        return -1;
    }

    // check the port is valid
    if (port & 0x7 == 0) || (port & ! 0x7 != 0) {
        return -1;
    }

    // println!("start: {:#x}, len: {:#x}, port: {:#x}", start, len, port);
    

    // Align the length to page size
    let ved = VirtAddr::from(VirtAddr(start + len).ceil());

    // println!("vst: {:#x}, ved: {:#x}", vst.0, ved.0);

    // check the length is valid
    if check_rest_memory(ved.0 - vst.0) == false {
        return -1;
    }

    // get the memory set of current task
    if do_memory_map(vst, ved, port) == -1 {
        return -1;
    }

    

    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    let vst = VirtAddr(start);

    // check the start address is valid
    if vst.page_offset() != 0 {
        return -1;
    }

    // Align the length to page size
    let ved = VirtAddr::from(VirtAddr(start + len).ceil());

    if do_memory_unmap(vst, ved) == -1 {
        return -1;
    }

    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
