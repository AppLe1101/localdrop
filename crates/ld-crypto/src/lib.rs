use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap, fs, path::{Path, PathBuf},
};

pub type Fingerprint = [u8; 32];

pub fn fingerprint(cert_der: &[u8]) -> Fingerprint {
    *blake3::hash(cert_der).as_bytes()
}

pub fn fp_hex(fp: &Fingerprint) -> String {
    fp.iter().map(|b| format!("{b:02x}")).collect()
}

// first 16 hex-symbols displayed to user
pub fn fp_short(fp: &Fingerprint) -> String {
    fp_hex(fp)[..16].to_string()
}

// Confirmation code: same on 2 devices
pub fn short_code(a: &Fingerprint, b: &Fingerprint) -> String {
    let (x, y) = if a <= b {(a, b)} else {(b, a)};
    let mut h = blake3::Hasher::new();
    h.update(b"localdrop-code-v1");
    h.update(x);
    h.update(y);
    let d = h.finalize();
    let n = u64::from_le_bytes(d.as_bytes()[..8].try_into().unwrap()) % 100_000_000;
    format!("{:04} {:04}", n / 10_000, n % 10_000)
}

// Device identity
pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub fingerprint: Fingerprint,
}

impl Identity {
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let cert_p = dir.join("cert.der");
        let key_p = dir.join("key.der");

        let (cert_der, key_der) = if cert_p.exists() && key_p.exists() {
            (fs::read(&cert_p)?, fs::read(&key_p)?)
        } else {
            let ck = rcgen::generate_simple_self_signed(vec!["localdrop".to_string()])?;
            let cert = ck.cert.der().to_vec();
            let key = ck.key_pair.serialize_der();
            fs::write(&cert_p, &cert)?;
            write_private(&key_p, &key)?;
            (cert, key)
        };
        let fp = fingerprint(&cert_der);
        Ok(Self { cert_der, key_der, fingerprint: fp })
    }

    pub fn cert(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.cert_der.clone())
    }

    pub fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }
}

fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

// Trusted devices
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Device {
    pub name: String,
    pub trusted: bool,
    #[serde(default)]
    pub renamed: bool,
}

pub struct TrustStore {
    path: PathBuf,
    devices: BTreeMap<String, Device>,
}

impl TrustStore {
    pub fn load(path: PathBuf) -> Result<Self> {
        let devices = if path.exists() {
            serde_json::from_slice(&fs::read(&path)?)
                .context("file of trusted devices corrupted")?
        } else {
            BTreeMap::new()
        };
        Ok(Self {path, devices})
    }

    fn save(&self) -> Result<()> {
        fs::write(&self.path, serde_json::to_vec_pretty(&self.devices)?)?;
        Ok(())
    }

    pub fn get(&self, fp: &Fingerprint) -> Option<&Device> {
        self.devices.get(&fp_hex(fp))
    }

    // add or update device
    pub fn upsert(&mut self, fp: &Fingerprint, name: &str, trusted: bool) -> Result<()> {
        let e = self
            .devices
            .entry(fp_hex(fp))
            .or_insert(Device { name: name.to_string(), trusted: false, renamed: false });
        if !e.renamed {
            e.name = name.to_string();
        }
        e.trusted = e.trusted || trusted;
        self.save()
    }

    // device with same name but with other fingerprint
    pub fn name_conflict(&self, name: &str, fp: &Fingerprint) -> bool {
        let me = fp_hex(fp);
        self.devices.iter().any(|(k, d)| d.name == name && *k != me)
    }
    pub fn list(&self) -> impl Iterator<Item = (&String, &Device)> {
        self.devices.iter()
    }

    // delete devices, witch fingerprint starts with prefix
    pub fn remove_by_prefix(&mut self, prefix: &str) -> Result<usize> {
        let keys: Vec<String> = self
            .devices
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        for k in &keys {
            self.devices.remove(k);
        }
        self.save()?;
        Ok(keys.len())
    }

    // turn on or delete trust to device
    pub fn set_trusted(&mut self, key: &str, trusted: bool) -> Result<bool> {
        match self.devices.get_mut(key) {
            Some(d) => {
                d.trusted = trusted;
                self.save()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn rename(&mut self, key: &str, name: &str) -> Result<bool> {
        match self.devices.get_mut(key) {
            Some(d) => {
                d.name = name.to_string();
                d.renamed = true;
                self.save()?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}