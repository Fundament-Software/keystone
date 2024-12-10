#[cfg(target_os = "windows")]
unsafe fn get_mac() -> Option<u64> {
    use std::mem::MaybeUninit;
    use windows_sys::Win32::{Foundation::ERROR_SUCCESS, NetworkManagement::IpHelper};

    let mut info: [MaybeUninit<IpHelper::IP_ADAPTER_INFO>; 32] =
        [const { MaybeUninit::uninit() }; 32];
    let mut len: u32 = size_of_val(&info) as u32;

    let status = IpHelper::GetAdaptersInfo(info[0].as_mut_ptr(), &mut len);
    if status != ERROR_SUCCESS {
        return None;
    }

    let mut all = Vec::new();
    let mut padapter = info[0].assume_init_mut() as *mut IpHelper::IP_ADAPTER_INFO;
    let mut i = 0;
    while !padapter.is_null() {
        (*padapter).Address[(*padapter).AddressLength as usize..].fill(0);
        all.push(u64::from_le_bytes((*padapter).Address));
        padapter = (*padapter).Next;
        info[i].assume_init_drop();
        i += 1;
    }

    // We only need one MAC address but we have to be absolutely sure it's the same one every time.
    all.sort();
    all.first().copied()
}

fn get_cpuid() -> u128 {
    let r = unsafe { std::arch::x86_64::__cpuid(0) };
    r.eax as u128 | (r.ebx as u128) << 32 | (r.ecx as u128) << 64 | (r.edx as u128) << 96
}

pub struct SnowflakeSource {
    machine: u64,
    instance: u64,
    sequence: std::sync::atomic::AtomicU32,
}

impl Default for SnowflakeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SnowflakeSource {
    pub fn new() -> Self {
        Self {
            machine: Self::gen_machine_id(),
            sequence: std::sync::atomic::AtomicU32::new(0),
            instance: capnpc::generate_random_id(),
        }
    }

    pub fn machine_id(&self) -> u64 {
        self.machine
    }

    pub fn instance_id(&self) -> u64 {
        self.instance
    }

    pub fn get(&self) -> u64 {
        caplog::as_snowflake(
            self.sequence
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            chrono::Utc::now().timestamp_millis() as u64,
        )
    }

    #[cfg(target_os = "windows")]
    pub fn gen_machine_id() -> u64 {
        unsafe {
            get_mac().unwrap_or_else(|| {
                let id = get_cpuid();
                (id >> 64) as u64 | id as u64
            })
        }
    }

    #[cfg(target_os = "linux")]
    pub fn gen_machine_id() -> u64 {
        if let Ok(key) = sshkeys::PublicKey::from_path("/etc/ssh/ssh_host_ed25519_key.pub") {
            if let sshkeys::PublicKeyKind::Ed25519(kind) = key.kind {
                return u64::from_le_bytes(kind.key[0..8].try_into().unwrap());
            }
        }
        if let Ok(key) = sshkeys::PublicKey::from_path("/etc/ssh/ssh_host_rsa_key.pub") {
            if let sshkeys::PublicKeyKind::Rsa(kind) = key.kind {
                return u64::from_le_bytes(kind.e[0..8].try_into().unwrap());
            }
        }

        let id = get_cpuid();
        (id >> 64) as u64 | id as u64
    }
}
