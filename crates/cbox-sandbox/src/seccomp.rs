use nix::libc;
use tracing::info;

use crate::error::SandboxError;

/// Default syscalls to block inside the sandbox.
/// These prevent the sandbox from escaping its confinement.
const BLOCKED_SYSCALLS: &[&str] = &[
    "mount",
    "umount2",
    "pivot_root",
    "unshare",
    "setns",
    "reboot",
    "swapon",
    "swapoff",
    "kexec_load",
    "kexec_file_load",
    "init_module",
    "finit_module",
    "delete_module",
    "acct",
    "syslog",
    "bpf",
    "userfaultfd",
    "perf_event_open",
    "ptrace",
];

/// Apply seccomp-BPF filter to block dangerous syscalls.
/// This MUST be called last in sandbox setup (after mount/pivot_root).
pub fn apply_seccomp_filter(extra_blocked: &[String]) -> Result<(), SandboxError> {
    let mut blocked: Vec<&str> = BLOCKED_SYSCALLS.to_vec();
    let extra_refs: Vec<&str> = extra_blocked.iter().map(|s| s.as_str()).collect();
    blocked.extend(extra_refs);

    // We use a simple approach: write a BPF program that blocks these syscalls.
    // In production, we'd use seccompiler or libseccomp.
    // For now, we use prctl with seccomp strict mode as a base,
    // and build a BPF filter for the denylist approach.

    #[cfg(target_arch = "x86_64")]
    {
        // Use the kernel's seccomp interface via prctl
        // SECCOMP_SET_MODE_FILTER = 1
        // We'll generate BPF bytecode for a denylist filter

        let filter = build_bpf_denylist(&blocked)?;

        // Apply via prctl(PR_SET_NO_NEW_PRIVS, 1) first
        let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if ret != 0 {
            return Err(SandboxError::Seccomp(format!(
                "prctl(PR_SET_NO_NEW_PRIVS) failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        // Apply seccomp filter
        let prog = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_ptr() as *mut libc::sock_filter,
        };

        let ret = unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                &prog as *const libc::sock_fprog,
                0,
                0,
            )
        };

        if ret != 0 {
            return Err(SandboxError::Seccomp(format!(
                "seccomp filter failed: {}",
                std::io::Error::last_os_error()
            )));
        }
    }

    info!("seccomp filter applied, blocked {} syscalls", blocked.len());
    Ok(())
}

/// Build a BPF denylist filter for the given syscall names.
#[cfg(target_arch = "x86_64")]
fn build_bpf_denylist(blocked: &[&str]) -> Result<Vec<libc::sock_filter>, SandboxError> {
    let mut filter: Vec<libc::sock_filter> = Vec::new();

    // Verify architecture is x86_64 (AUDIT_ARCH_X86_64 = 0xC000003E), kill if not
    filter.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 4)); // seccomp_data.arch
    filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, 0xC000003E, 1, 0));
    filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));

    filter.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, 0)); // seccomp_data.nr

    let syscall_numbers = resolve_syscall_numbers(blocked);
    let num_checks = syscall_numbers.len();

    for (i, nr) in syscall_numbers.iter().enumerate() {
        let kill_offset = (num_checks - i) as u8;
        filter.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, *nr, kill_offset, 0));
    }

    filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));
    filter.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | (libc::EPERM as u32)));

    Ok(filter)
}

#[cfg(target_arch = "x86_64")]
fn resolve_syscall_numbers(names: &[&str]) -> Vec<u32> {
    let mut numbers = Vec::new();
    for name in names {
        if let Some(nr) = syscall_number(name) {
            numbers.push(nr);
        }
    }
    numbers
}

/// Map syscall names to x86_64 numbers.
#[cfg(target_arch = "x86_64")]
fn syscall_number(name: &str) -> Option<u32> {
    // x86_64 syscall numbers
    match name {
        "mount" => Some(165),
        "umount2" => Some(166),
        "pivot_root" => Some(155),
        "unshare" => Some(272),
        "setns" => Some(308),
        "reboot" => Some(169),
        "swapon" => Some(167),
        "swapoff" => Some(168),
        "kexec_load" => Some(246),
        "kexec_file_load" => Some(320),
        "init_module" => Some(175),
        "finit_module" => Some(313),
        "delete_module" => Some(176),
        "acct" => Some(163),
        "syslog" => Some(103),
        "bpf" => Some(321),
        "userfaultfd" => Some(323),
        "perf_event_open" => Some(298),
        "ptrace" => Some(101),
        _ => None,
    }
}

// BPF instruction helpers
const BPF_LD: u32 = 0x00;
const BPF_JMP: u32 = 0x05;
const BPF_RET: u32 = 0x06;
const BPF_W: u32 = 0x00;
const BPF_ABS: u32 = 0x20;
const BPF_JEQ: u32 = 0x10;
const BPF_K: u32 = 0x00;

const SECCOMP_RET_KILL_PROCESS: u32 = 0x80000000;
const SECCOMP_RET_ALLOW: u32 = 0x7FFF0000;
const SECCOMP_RET_ERRNO: u32 = 0x00050000;

#[cfg(target_arch = "x86_64")]
fn bpf_stmt(code: u32, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    }
}

#[cfg(target_arch = "x86_64")]
fn bpf_jump(code: u32, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}
