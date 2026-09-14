//! Small CoreFoundation/SystemConfiguration binding. No subprocess shell or lossy parsing.
use super::{Backend, Change, Dict, Plist, Service};
use anyhow::{bail, ensure, Result};
use std::{
    ffi::{c_char, c_void},
    ptr,
};
type Ref = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(v: Ref);
    fn CFRetain(v: Ref) -> Ref;
    fn CFGetTypeID(v: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFDataGetTypeID() -> usize;
    fn CFStringCreateWithBytes(a: Ref, b: *const u8, n: isize, encoding: u32, external: u8) -> Ref;
    fn CFStringGetLength(s: Ref) -> isize;
    fn CFStringGetMaximumSizeForEncoding(n: isize, encoding: u32) -> isize;
    fn CFStringGetCString(s: Ref, buffer: *mut c_char, n: isize, encoding: u32) -> u8;
    fn CFNumberIsFloatType(v: Ref) -> u8;
    fn CFNumberGetValue(v: Ref, kind: i32, out: *mut c_void) -> u8;
    fn CFNumberCreate(a: Ref, kind: i32, value: *const c_void) -> Ref;
    fn CFBooleanGetValue(v: Ref) -> u8;
    fn CFArrayGetCount(v: Ref) -> isize;
    fn CFArrayGetValueAtIndex(v: Ref, i: isize) -> Ref;
    fn CFArrayCreate(a: Ref, values: *const Ref, count: isize, callbacks: Ref) -> Ref;
    fn CFDictionaryGetCount(v: Ref) -> isize;
    fn CFDictionaryGetKeysAndValues(v: Ref, keys: *mut Ref, values: *mut Ref);
    fn CFDictionaryCreate(
        a: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        k: Ref,
        v: Ref,
    ) -> Ref;
    fn CFDataGetLength(v: Ref) -> isize;
    fn CFDataGetBytePtr(v: Ref) -> *const u8;
    fn CFDataCreate(a: Ref, bytes: *const u8, count: isize) -> Ref;
    static kCFBooleanTrue: Ref;
    static kCFBooleanFalse: Ref;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    static kCFTypeArrayCallBacks: u8;
}
#[link(name = "SystemConfiguration", kind = "framework")]
extern "C" {
    fn SCPreferencesCreate(a: Ref, name: Ref, prefs: Ref) -> Ref;
    fn SCPreferencesSynchronize(p: Ref);
    fn SCPreferencesLock(p: Ref, wait: u8) -> u8;
    fn SCPreferencesUnlock(p: Ref) -> u8;
    fn SCPreferencesCommitChanges(p: Ref) -> u8;
    fn SCPreferencesApplyChanges(p: Ref) -> u8;
    fn SCPreferencesPathGetValue(p: Ref, path: Ref) -> Ref;
    fn SCPreferencesPathSetValue(p: Ref, path: Ref, value: Ref) -> u8;
    fn SCPreferencesPathRemoveValue(p: Ref, path: Ref) -> u8;
    fn SCNetworkSetCopyCurrent(p: Ref) -> Ref;
    fn SCNetworkSetCopyServices(s: Ref) -> Ref;
    fn SCNetworkServiceGetEnabled(s: Ref) -> u8;
    fn SCNetworkServiceGetName(s: Ref) -> Ref;
    fn SCNetworkServiceGetServiceID(s: Ref) -> Ref;
    fn SCNetworkServiceGetInterface(s: Ref) -> Ref;
    fn SCNetworkInterfaceGetInterfaceType(i: Ref) -> Ref;
    fn SCDynamicStoreCopyProxies(store: Ref) -> Ref;
    fn SCError() -> i32;
}
struct Owned(Ref);
impl Owned {
    fn new(v: Ref) -> Result<Self> {
        ensure!(
            !v.is_null(),
            "macOS configuration object unavailable (code {})",
            unsafe { SCError() }
        );
        Ok(Self(v))
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
const UTF8: u32 = 0x08000100;
fn string(s: &str) -> Result<Owned> {
    Owned::new(unsafe {
        CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0)
    })
}
fn read_string(v: Ref) -> Result<String> {
    ensure!(
        !v.is_null() && unsafe { CFGetTypeID(v) == CFStringGetTypeID() },
        "Expected macOS string"
    );
    let n = unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(v), UTF8) } + 1;
    ensure!(n > 0 && n < 4 * 1024 * 1024, "macOS string too large");
    let mut bytes = vec![0; n as usize];
    ensure!(
        unsafe { CFStringGetCString(v, bytes.as_mut_ptr().cast(), n, UTF8) } != 0,
        "Cannot decode macOS string"
    );
    bytes.truncate(bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len()));
    Ok(String::from_utf8(bytes)?)
}
fn decode(v: Ref, depth: u32) -> Result<Plist> {
    ensure!(
        depth < 32 && !v.is_null(),
        "Unsupported macOS preference value"
    );
    unsafe {
        let t = CFGetTypeID(v);
        if t == CFStringGetTypeID() {
            return Ok(Plist::String(read_string(v)?));
        }
        if t == CFBooleanGetTypeID() {
            return Ok(Plist::Bool(CFBooleanGetValue(v) != 0));
        }
        if t == CFNumberGetTypeID() {
            if CFNumberIsFloatType(v) != 0 {
                let mut n = 0f64;
                ensure!(
                    CFNumberGetValue(v, 6, (&mut n as *mut f64).cast()) != 0 && n.is_finite(),
                    "Invalid real preference"
                );
                return Ok(Plist::Real(n));
            }
            let mut n = 0i64;
            ensure!(
                CFNumberGetValue(v, 4, (&mut n as *mut i64).cast()) != 0,
                "Invalid integer preference"
            );
            return Ok(Plist::Int(n));
        }
        if t == CFDataGetTypeID() {
            let n = CFDataGetLength(v);
            ensure!(
                (0..4 * 1024 * 1024).contains(&n),
                "Preference data too large"
            );
            return Ok(Plist::Data(if n == 0 {
                vec![]
            } else {
                std::slice::from_raw_parts(CFDataGetBytePtr(v), n as usize).to_vec()
            }));
        }
        if t == CFArrayGetTypeID() {
            let n = CFArrayGetCount(v);
            ensure!((0..10000).contains(&n), "Preference array too large");
            return Ok(Plist::Array(
                (0..n)
                    .map(|i| decode(CFArrayGetValueAtIndex(v, i), depth + 1))
                    .collect::<Result<_>>()?,
            ));
        }
        if t == CFDictionaryGetTypeID() {
            let n = CFDictionaryGetCount(v);
            ensure!((0..10000).contains(&n), "Preference dictionary too large");
            let mut keys = vec![ptr::null(); n as usize];
            let mut vals = keys.clone();
            CFDictionaryGetKeysAndValues(v, keys.as_mut_ptr(), vals.as_mut_ptr());
            let mut d = Dict::new();
            for (k, v) in keys.into_iter().zip(vals) {
                d.insert(read_string(k)?, decode(v, depth + 1)?);
            }
            return Ok(Plist::Dict(d));
        }
    }
    bail!("Unsupported plist type; refusing lossy proxy backup")
}
fn encode(v: &Plist) -> Result<Owned> {
    unsafe {
        match v {
            Plist::String(s) => string(s),
            Plist::Bool(b) => {
                Owned::new(CFRetain(if *b { kCFBooleanTrue } else { kCFBooleanFalse }))
            }
            Plist::Int(n) => Owned::new(CFNumberCreate(ptr::null(), 4, (n as *const i64).cast())),
            Plist::Real(n) => Owned::new(CFNumberCreate(ptr::null(), 6, (n as *const f64).cast())),
            Plist::Data(b) => Owned::new(CFDataCreate(ptr::null(), b.as_ptr(), b.len() as isize)),
            Plist::Array(a) => {
                let owned = a.iter().map(encode).collect::<Result<Vec<_>>>()?;
                let raw = owned.iter().map(|v| v.0).collect::<Vec<_>>();
                Owned::new(CFArrayCreate(
                    ptr::null(),
                    raw.as_ptr(),
                    raw.len() as isize,
                    ptr::addr_of!(kCFTypeArrayCallBacks).cast(),
                ))
            }
            Plist::Dict(d) => {
                let keys = d.keys().map(|s| string(s)).collect::<Result<Vec<_>>>()?;
                let vals = d.values().map(encode).collect::<Result<Vec<_>>>()?;
                let k = keys.iter().map(|v| v.0).collect::<Vec<_>>();
                let v = vals.iter().map(|v| v.0).collect::<Vec<_>>();
                Owned::new(CFDictionaryCreate(
                    ptr::null(),
                    k.as_ptr(),
                    v.as_ptr(),
                    k.len() as isize,
                    ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
                    ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
                ))
            }
        }
    }
}

pub struct MacBackend {
    prefs: Owned,
}
impl MacBackend {
    pub fn new() -> Result<Self> {
        let name = string("sing system proxy")?;
        Ok(Self {
            prefs: Owned::new(unsafe { SCPreferencesCreate(ptr::null(), name.0, ptr::null()) })?,
        })
    }
    fn path(id: &str) -> Result<Owned> {
        ensure!(
            !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "Invalid network service identifier"
        );
        string(&format!("/NetworkServices/{id}/Proxies"))
    }
    fn read_locked(&self, id: &str) -> Result<Option<Dict>> {
        let path = Self::path(id)?;
        let service = string(&format!("/NetworkServices/{id}"))?;
        if unsafe { SCPreferencesPathGetValue(self.prefs.0, service.0) }.is_null() {
            let code = unsafe { SCError() };
            if code == 1004 {
                return Err(super::RemovedService.into());
            }
            bail!("Cannot read network service (code {code})");
        }
        let raw = unsafe { SCPreferencesPathGetValue(self.prefs.0, path.0) };
        if raw.is_null() {
            ensure!(
                unsafe { SCError() } == 1004,
                "Cannot read proxy preferences (code {})",
                unsafe { SCError() }
            );
            return Ok(None);
        }
        let Plist::Dict(d) = decode(raw, 0)? else {
            bail!("Proxy preference is not a dictionary")
        };
        Ok(Some(d))
    }
}
struct Lock(Ref);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            SCPreferencesUnlock(self.0);
        }
    }
}
impl Backend for MacBackend {
    fn services(&mut self) -> Result<Vec<Service>> {
        unsafe {
            SCPreferencesSynchronize(self.prefs.0);
        }
        let set = Owned::new(unsafe { SCNetworkSetCopyCurrent(self.prefs.0) })?;
        let services = Owned::new(unsafe { SCNetworkSetCopyServices(set.0) })?;
        let mut result = vec![];
        for i in 0..unsafe { CFArrayGetCount(services.0) } {
            let s = unsafe { CFArrayGetValueAtIndex(services.0, i) };
            if unsafe { SCNetworkServiceGetEnabled(s) } == 0 {
                continue;
            }
            let interface = unsafe { SCNetworkServiceGetInterface(s) };
            if interface.is_null() {
                continue;
            }
            let kind = read_string(unsafe { SCNetworkInterfaceGetInterfaceType(interface) })?;
            if !["Ethernet", "IEEE80211", "Bridge", "Bond", "VLAN"].contains(&kind.as_str()) {
                continue;
            }
            let id = read_string(unsafe { SCNetworkServiceGetServiceID(s) })?;
            let name = crate::model::clean(&read_string(unsafe { SCNetworkServiceGetName(s) })?);
            let proxies = self.read_locked(&id)?;
            result.push(Service { id, name, proxies });
        }
        Ok(result)
    }
    fn read(&mut self, id: &str) -> Result<Option<Dict>> {
        unsafe {
            SCPreferencesSynchronize(self.prefs.0);
        }
        self.read_locked(id)
    }
    fn transact(&mut self, changes: &[Change]) -> Result<()> {
        ensure!(
            unsafe { libc::geteuid() } == 0,
            "Administrator authorization is required"
        );
        unsafe {
            SCPreferencesSynchronize(self.prefs.0);
        }
        ensure!(
            unsafe { SCPreferencesLock(self.prefs.0, 0) } != 0,
            "macOS network settings are busy (code {}); no changes applied",
            unsafe { SCError() }
        );
        let _lock = Lock(self.prefs.0);
        unsafe {
            SCPreferencesSynchronize(self.prefs.0);
        }
        for c in changes {
            ensure!(
                self.read_locked(&c.id)? == c.expected,
                "Network settings changed concurrently; refusing to overwrite them"
            );
        }
        for c in changes {
            let path = Self::path(&c.id)?;
            let ok = if let Some(d) = &c.replacement {
                let value = encode(&Plist::Dict(d.clone()))?;
                unsafe { SCPreferencesPathSetValue(self.prefs.0, path.0, value.0) }
            } else {
                unsafe { SCPreferencesPathRemoveValue(self.prefs.0, path.0) }
            };
            ensure!(
                ok != 0,
                "Could not stage proxy settings (code {})",
                unsafe { SCError() }
            );
        }
        ensure!(
            unsafe { SCPreferencesCommitChanges(self.prefs.0) } != 0,
            "Could not commit proxy settings (code {})",
            unsafe { SCError() }
        );
        ensure!(
            unsafe { SCPreferencesApplyChanges(self.prefs.0) } != 0,
            "Preferences committed but macOS failed to apply them (code {}); recovery is required",
            unsafe { SCError() }
        );
        Ok(())
    }
}
pub fn effective(port: u16) -> Result<bool> {
    let d = Owned::new(unsafe { SCDynamicStoreCopyProxies(ptr::null()) })?;
    let Plist::Dict(d) = decode(d.0, 0)? else {
        bail!("Effective proxy state unavailable")
    };
    Ok(super::fully_local(&d, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plist_roundtrip_preserves_nested_values() {
        let value = Plist::Dict(Dict::from([
            ("Enabled".into(), Plist::Int(1)),
            (
                "Nested".into(),
                Plist::Array(vec![
                    Plist::Bool(true),
                    Plist::String("日本".into()),
                    Plist::Data(vec![1, 2, 3]),
                    Plist::Real(1.5),
                ]),
            ),
        ]));
        let native = encode(&value).unwrap();
        assert_eq!(decode(native.0, 0).unwrap(), value);
    }
    #[test]
    fn service_path_rejects_injection() {
        assert!(MacBackend::path("../../Other").is_err());
        assert!(MacBackend::path("UUID-1234").is_ok());
    }
    #[test]
    #[ignore = "Read-only macOS SystemConfiguration access; never writes preferences"]
    fn read_only_native_inventory() {
        let services = MacBackend::new().unwrap().services().unwrap();
        println!("Readable enabled physical services: {}", services.len());
        for s in services {
            if let Some(p) = s.proxies {
                let native = encode(&Plist::Dict(p.clone())).unwrap();
                assert_eq!(decode(native.0, 0).unwrap(), Plist::Dict(p));
            }
        }
        let _ = effective(2080).unwrap();
    }
}
