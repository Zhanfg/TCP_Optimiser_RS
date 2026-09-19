use serde::Serialize;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Bbr2ProviderStatus {
    pub provider: String,
    pub native_available: bool,
    pub bpf_struct_ops_candidate: bool,
    pub bpf_syscall_present: bool,
    pub vmlinux_btf: bool,
    pub tcp_congestion_ops_btf: bool,
    pub struct_ops_wrapper_btf: bool,
    pub tcp_sock_btf: bool,
    pub rate_sample_btf: bool,
    pub kernel_release: String,
    pub note: String,
}

pub fn probe() -> Bbr2ProviderStatus {
    let native_available = crate::sysctl::algo_available("bbr2").unwrap_or(false);
    let kernel_release = kernel_release().unwrap_or_else(|| "unknown".to_string());
    let bpf_syscall_present = bpf_syscall_present();

    let btf_path = Path::new("/sys/kernel/btf/vmlinux");
    let btf = fs::read(btf_path).ok();
    let vmlinux_btf = btf.is_some();
    let tcp_congestion_ops_btf = btf
        .as_deref()
        .is_some_and(|bytes| contains_btf_name(bytes, b"tcp_congestion_ops"));
    let struct_ops_wrapper_btf = btf.as_deref().is_some_and(|bytes| {
        contains_btf_name(bytes, b"bpf_struct_ops_tcp_congestion_ops")
    });
    let tcp_sock_btf = btf
        .as_deref()
        .is_some_and(|bytes| contains_btf_name(bytes, b"tcp_sock"));
    let rate_sample_btf = btf
        .as_deref()
        .is_some_and(|bytes| contains_btf_name(bytes, b"rate_sample"));

    let bpf_struct_ops_candidate = !native_available
        && bpf_syscall_present
        && vmlinux_btf
        && tcp_congestion_ops_btf
        && struct_ops_wrapper_btf
        && tcp_sock_btf
        && rate_sample_btf;

    let (provider, note) = if native_available {
        (
            "native",
            "Kernel already exposes bbr2; no bundled compiler or compatibility payload is needed.",
        )
    } else if bpf_struct_ops_candidate {
        (
            "bpf-struct-ops-candidate",
            "Core BPF/BTF prerequisites are visible. A BBRv2-compatible struct_ops program still requires verifier and behavioral validation before it can be enabled.",
        )
    } else {
        (
            "unavailable",
            "Stock-kernel BBRv2 cannot be supplied as a standalone tcp_bbr2.ko because Google BBRv2 depends on TCP-core changes. Use native/patched-kernel BBRv2 or a validated BPF provider.",
        )
    };

    Bbr2ProviderStatus {
        provider: provider.to_string(),
        native_available,
        bpf_struct_ops_candidate,
        bpf_syscall_present,
        vmlinux_btf,
        tcp_congestion_ops_btf,
        struct_ops_wrapper_btf,
        tcp_sock_btf,
        rate_sample_btf,
        kernel_release,
        note: note.to_string(),
    }
}

fn contains_btf_name(bytes: &[u8], name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    bytes
        .windows(name.len() + 1)
        .any(|window| window.starts_with(name) && window[name.len()] == 0)
}

fn kernel_release() -> Option<String> {
    let output = Command::new("uname").arg("-r").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn bpf_syscall_present() -> bool {
    // An invalid command should yield EINVAL/EPERM on a kernel that implements
    // bpf(2). ENOSYS means the syscall is absent entirely.
    let result = unsafe {
        libc::syscall(
            libc::SYS_bpf as libc::c_long,
            u32::MAX as libc::c_long,
            std::ptr::null::<libc::c_void>(),
            0usize,
        )
    };
    if result >= 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS)
}

#[cfg(test)]
mod tests {
    use super::contains_btf_name;

    #[test]
    fn finds_nul_terminated_btf_names_only() {
        let bytes =
            b"prefix\0tcp_sock\0rate_sample\0tcp_congestion_ops\0bpf_struct_ops_tcp_congestion_ops\0suffix";
        assert!(contains_btf_name(bytes, b"tcp_sock"));
        assert!(contains_btf_name(bytes, b"rate_sample"));
        assert!(contains_btf_name(bytes, b"tcp_congestion_ops"));
        assert!(contains_btf_name(
            bytes,
            b"bpf_struct_ops_tcp_congestion_ops"
        ));
        assert!(!contains_btf_name(bytes, b"tcp"));
        assert!(!contains_btf_name(bytes, b"missing"));
    }
}
