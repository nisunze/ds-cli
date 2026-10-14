//! Owns descendants of fixed native tasks; no executable or argv surface.
use std::io;
use std::process::{Child, Command};
#[cfg(unix)]
pub(crate) struct Tree {
    group: libc::pid_t,
    terminated: bool,
}
#[cfg(unix)]
impl Tree {
    pub(crate) fn prepare(command: &mut Command) {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        #[cfg(target_os = "linux")]
        {
            let parent = std::process::id() as libc::pid_t;
            // Both syscalls are async-signal-safe and touch no Rust state.
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(io::Error::from_raw_os_error(libc::ECHILD));
                    }
                    Ok(())
                });
            }
        }
    }
    pub(crate) fn attach(child: &mut Child) -> io::Result<Self> {
        Ok(Self {
            group: child.id() as libc::pid_t,
            terminated: false,
        })
    }
    /// Observe Linux exit without reaping the group leader. Keeping its PID
    /// reserved until group cleanup prevents signalling a newly reused group.
    pub(crate) fn try_wait(
        &self,
        child: &mut Child,
    ) -> io::Result<Option<std::process::ExitStatus>> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::ExitStatusExt;
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            if unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            if unsafe { info.si_pid() } == 0 {
                return Ok(None);
            }
            let status = unsafe { info.si_status() };
            let raw = match info.si_code {
                libc::CLD_EXITED => status << 8,
                libc::CLD_KILLED => status,
                libc::CLD_DUMPED => status | 0x80,
                _ => return Err(io::Error::other("unexpected native task wait status")),
            };
            Ok(Some(std::process::ExitStatus::from_raw(raw)))
        }
        #[cfg(not(target_os = "linux"))]
        child.try_wait()
    }
    pub(crate) fn terminate(&mut self) {
        if !self.terminated {
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
            self.terminated = true;
        }
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::{io::AsRawHandle, process::CommandExt};
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
            },
        },
    };
    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub(crate) struct Tree {
        job: Option<Owned>,
    }
    impl Tree {
        pub(crate) fn prepare(command: &mut Command) {
            command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        }
        pub(crate) fn attach(child: &mut Child) -> io::Result<Self> {
            unsafe {
                let job = CreateJobObjectW(ptr::null(), ptr::null());
                if job.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = Owned(job);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of_val(&limits) as u32,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
                if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if snapshot == INVALID_HANDLE_VALUE {
                    return Err(io::Error::last_os_error());
                }
                let snapshot = Owned(snapshot);
                let mut entry: THREADENTRY32 = std::mem::zeroed();
                entry.dwSize = std::mem::size_of_val(&entry) as u32;
                if Thread32First(snapshot.0, &mut entry) == 0 {
                    return Err(io::Error::last_os_error());
                }
                loop {
                    if entry.th32OwnerProcessID == child.id() {
                        let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                        if thread.is_null() {
                            return Err(io::Error::last_os_error());
                        }
                        let thread = Owned(thread);
                        if ResumeThread(thread.0) == u32::MAX {
                            return Err(io::Error::last_os_error());
                        }
                        return Ok(Self { job: Some(job) });
                    }
                    if Thread32Next(snapshot.0, &mut entry) == 0 {
                        break;
                    }
                }
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "suspended native task has no initial thread",
                ))
            }
        }
        pub(crate) fn terminate(&mut self) {
            if let Some(job) = self.job.take() {
                unsafe {
                    TerminateJobObject(job.0, 1);
                }
                drop(job);
            }
        }
        pub(crate) fn try_wait(
            &self,
            child: &mut Child,
        ) -> io::Result<Option<std::process::ExitStatus>> {
            child.try_wait()
        }
    }
}
#[cfg(windows)]
pub(crate) use windows::Tree;
impl Drop for Tree {
    fn drop(&mut self) {
        self.terminate();
    }
}
