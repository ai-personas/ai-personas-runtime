//! Unix vault operations are anchored to validated directory descriptors.
//! The immediate parent must already exist, be owned by the effective UID,
//! and have mode 0700. Lock files persist and must not be removed by callers.
//! Cooperative writers hold the account lock through refresh and replacement.
//! A process with the same UID can already read or alter these credentials;
//! this vault is not a security boundary against that process or root.

use super::{
    finish_tokens, form, new_host_id, send_tokens, valid_host_id, AuthError, Clock,
    CredentialRecord, Entropy, GrantBinding, IdTokenVerifier, PendingRegistration,
    StoredCredential, StoredRegistration, TokenTransport, RESOURCE, TOKEN_TIMEOUT,
};
use std::{
    ffi::{CStr, CString},
    fmt,
    fs::{File, Metadata},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path},
};

const HOST_NAME: &str = ".personas-host-id";
const HOST_FILE: &CStr = c".personas-host-id";
const LOCK_SUFFIX: &str = ".lock";
const REGISTRATION_SUFFIX: &str = ".registration";
const REFRESH_SUFFIX: &str = ".refresh";
const ONBOARDING_SUFFIX: &str = ".onboarding";
const TEMP_PREFIX: &str = ".personas-tmp-";
const READ_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;

pub struct Vault {
    directory: File,
    name: CString,
}

/// Owns both the anchored directory and the exclusive account lock. Dropping
/// the guard closes the lock descriptor. No credential read API exists on Vault.
pub struct LockedVault {
    directory: File,
    name: CString,
    _lock: File,
}

opaque_debug!(Vault, LockedVault);

// A saved or loaded handle must consult the anchored vault before handing off.
// This also revokes handles loaded before a failed refresh or a replacement.
pub(super) struct RecordSource {
    directory: File,
    name: CString,
}

impl RecordSource {
    pub(super) fn check(&self, expected: &StoredCredential) -> Result<(), AuthError> {
        let current = read_effective(&self.directory, &self.name)?.ok_or(AuthError::State)?;
        if current.refresh_uncertain.is_some() || current != *expected {
            return Err(AuthError::State);
        }
        if read_host(&self.directory)?.as_deref() != Some(expected.ext_agent_host_id.as_str()) {
            return Err(AuthError::Binding);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum CommitPhase {
    BeforeReplace,
    AfterReplace,
}

impl Vault {
    /// Explicit absolute app-owned file path only. This function does not create
    /// directories, inspect environment variables, or discover existing accounts.
    pub fn open(path: &Path) -> Result<Self, AuthError> {
        Self::open_checked(path)
    }

    // Onboarding excludes the active source checkout itself. Repository markers
    // in unrelated ancestors (for example a platform workspace mount) are not
    // treated as proof that this private leaf is exported by that repository.
    pub(super) fn open_for_onboarding(path: &Path) -> Result<Self, AuthError> {
        if path.starts_with(env!("CARGO_MANIFEST_DIR")) {
            return Err(AuthError::Vault);
        }
        Self::open_checked(path)
    }

    fn open_checked(path: &Path) -> Result<Self, AuthError> {
        if !path.is_absolute()
            || path.as_os_str().as_bytes().len() > 4096
            || path.as_os_str().as_bytes().last() == Some(&b'/')
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(AuthError::Vault);
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AuthError::Vault)?;
        if name.is_empty()
            || name.len() > 128
            || name == HOST_NAME
            || name.ends_with(LOCK_SUFFIX)
            || name.ends_with(REGISTRATION_SUFFIX)
            || name.ends_with(REFRESH_SUFFIX)
            || name.ends_with(ONBOARDING_SUFFIX)
            || name.starts_with(TEMP_PREFIX)
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(AuthError::Vault);
        }
        let name = CString::new(name).map_err(|_| AuthError::Vault)?;
        let directory = open_parent_checked(path.parent().ok_or(AuthError::Vault)?)?;
        drop(open_optional(&directory, &name)?);
        Ok(Self { directory, name })
    }

    /// Nonblocking exclusive flock. Busy is returned immediately on contention.
    pub fn lock(&self) -> Result<LockedVault, AuthError> {
        let directory = self.directory.try_clone().map_err(|_| AuthError::Vault)?;
        let mut lock_name = self.name.as_bytes().to_vec();
        lock_name.extend_from_slice(LOCK_SUFFIX.as_bytes());
        let lock_name = CString::new(lock_name).map_err(|_| AuthError::Vault)?;
        let lock = open_at(
            &directory,
            &lock_name,
            libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0o600,
        )
        .map_err(|_| AuthError::Vault)?;
        check_file(&lock)?;
        // SAFETY: the live File owns a valid descriptor; flock retains no pointer.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            return Err(if error.kind() == io::ErrorKind::WouldBlock {
                AuthError::Busy
            } else {
                AuthError::Vault
            });
        }
        let metadata = check_file(&lock)?;
        let linked = open_optional(&directory, &lock_name)?.ok_or(AuthError::Vault)?;
        let linked_metadata = check_file(&linked)?;
        if metadata.dev() != linked_metadata.dev() || metadata.ino() != linked_metadata.ino() {
            return Err(AuthError::Vault);
        }
        Ok(LockedVault {
            directory,
            name: self.name.clone(),
            _lock: lock,
        })
    }

    /// Persist or reuse the host-wide ID before the first redirect. Other account
    /// files in the same parent share it. A failed grant never rotates this ID.
    pub fn host_id(&self, entropy: &mut dyn Entropy) -> Result<String, AuthError> {
        self.lock()?.host_id(entropy)
    }
}

impl LockedVault {
    // Only the onboarding backend uses this secret-free, complete sidecar.
    // The account lock is retained through its read, claim, HTTP, and final save.
    pub(super) fn read_onboarding(&self) -> Result<Option<Vec<u8>>, AuthError> {
        let name = self.onboarding_name()?;
        open_optional(&self.directory, &name)?
            .map(|file| read_complete(file))
            .transpose()
    }

    pub(super) fn write_onboarding(&self, bytes: &[u8]) -> Result<(), AuthError> {
        let name = self.onboarding_name()?;
        // Do not overwrite an unprotected file, symlink, or multiply linked file.
        drop(open_optional(&self.directory, &name)?);
        Staged::stage(&self.directory, bytes)?.replace(&name)
    }

    fn onboarding_name(&self) -> Result<CString, AuthError> {
        let mut name = self.name.as_bytes().to_vec();
        name.extend_from_slice(ONBOARDING_SUFFIX.as_bytes());
        CString::new(name).map_err(|_| AuthError::Vault)
    }

    fn record_source(&self) -> Result<RecordSource, AuthError> {
        Ok(RecordSource {
            directory: self.directory.try_clone().map_err(|_| AuthError::Vault)?,
            name: self.name.clone(),
        })
    }

    fn read_record(&self) -> Result<Option<CredentialRecord>, AuthError> {
        let Some(data) = read_effective(&self.directory, &self.name)? else {
            return Ok(None);
        };
        let record = CredentialRecord::new(data, false);
        *record.source.lock().map_err(|_| AuthError::Vault)? = Some(self.record_source()?);
        Ok(Some(record))
    }

    fn check_pending_registration(
        &self,
        registration: &PendingRegistration,
    ) -> Result<(), AuthError> {
        registration.data.validate()?;
        if read_host(&self.directory)?.as_deref() != Some(registration.host_id()) {
            return Err(AuthError::Binding);
        }
        if self
            .load()?
            .as_ref()
            .is_some_and(|record| !registration.data.matches(&record.data))
        {
            return Err(AuthError::Binding);
        }
        Ok(())
    }

    /// Load a protected registration receipt. It confers no identity or grant.
    pub fn pending_registration(&self) -> Result<Option<PendingRegistration>, AuthError> {
        let registration = read_pending_registration(&self.directory, &self.name)?;
        if let Some(registration) = &registration {
            self.check_pending_registration(registration)?;
        }
        Ok(registration)
    }

    /// Retain only validated public registration metadata under this account lock.
    /// An existing registration, connected account, and persisted host must agree.
    pub fn retain_pending_registration(
        &self,
        registration: &PendingRegistration,
    ) -> Result<(), AuthError> {
        self.check_pending_registration(registration)?;
        if let Some(existing) = self.pending_registration()? {
            if existing.data != registration.data {
                return Err(AuthError::Binding);
            }
            return self
                .directory
                .sync_all()
                .map_err(|_| AuthError::CommitUncertain);
        }
        let bytes = serde_json::to_vec(&registration.data).map_err(|_| AuthError::Vault)?;
        let mut staged = Staged::new(&self.directory, &bytes)?;
        if self.pending_registration()?.is_some() {
            return Err(AuthError::State);
        }
        self.check_pending_registration(registration)?;
        let name = suffixed_name(&self.name, REGISTRATION_SUFFIX)?;
        if !staged.publish_new(&name)? {
            return Err(AuthError::State);
        }
        Ok(())
    }

    fn cleanup_registration(
        &self,
        expected: Option<&PendingRegistration>,
    ) -> Result<(), AuthError> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let actual = read_pending_registration(&self.directory, &self.name)?;
        if actual.as_ref().map(|registration| &registration.data) != Some(&expected.data) {
            return Err(AuthError::State);
        }
        remove_at(
            &self.directory,
            &suffixed_name(&self.name, REGISTRATION_SUFFIX)?,
        )?;
        self.directory
            .sync_all()
            .map_err(|_| AuthError::CommitUncertain)
    }

    fn check_host(&self, record: &CredentialRecord) -> Result<(), AuthError> {
        let host = read_host(&self.directory)?.ok_or(AuthError::Binding)?;
        if host != record.data.ext_agent_host_id {
            return Err(AuthError::Binding);
        }
        Ok(())
    }

    /// Load a structurally validated protected record. Expired records remain
    /// available for explicit refresh or reauthorization; handoff checks expiry.
    pub fn load(&self) -> Result<Option<CredentialRecord>, AuthError> {
        let record = self.read_record()?;
        if let Some(record) = &record {
            self.check_host(record)?;
        }
        Ok(record)
    }

    pub fn host_id(&self, entropy: &mut dyn Entropy) -> Result<String, AuthError> {
        let record = self.read_record()?;
        let registration = read_pending_registration(&self.directory, &self.name)?;
        let host = match read_host(&self.directory)? {
            Some(host) => host,
            None => {
                // A missing shared identity alongside credentials is corruption,
                // not permission to create a different host identity.
                if record.is_some() || registration.is_some() {
                    return Err(AuthError::Binding);
                }
                let candidate = new_host_id(entropy)?;
                if !valid_host_id(&candidate) {
                    return Err(AuthError::Vault);
                }
                let mut pending = Staged::new(&self.directory, candidate.as_bytes())?;
                let _created = pending.publish_new(HOST_FILE)?;
                // On a publication race, read the complete winner, never replace it.
                read_host(&self.directory)?.ok_or(AuthError::Vault)?
            }
        };
        if record
            .as_ref()
            .is_some_and(|record| record.data.ext_agent_host_id != host)
            || registration
                .as_ref()
                .is_some_and(|registration| registration.host_id() != host)
            || record
                .as_ref()
                .zip(registration.as_ref())
                .is_some_and(|(record, registration)| !registration.data.matches(&record.data))
        {
            return Err(AuthError::Binding);
        }
        // A losing creator can observe publication before the winner's fsync.
        // Sync here too, so every successful caller can safely open a redirect.
        self.directory
            .sync_all()
            .map_err(|_| AuthError::CommitUncertain)?;
        Ok(host)
    }

    /// Replace all credentials atomically, preserving registration identity.
    /// A fresh verified authorization has one-use replacement authority. Loaded
    /// records can only save identical, unfenced bytes. Registration cleanup
    /// follows durable credential commit; the two files are not one transaction.
    pub fn save(&self, record: &CredentialRecord) -> Result<(), AuthError> {
        self.save_with_hook(record, None, |_| Ok(()))
    }

    fn save_with_hook(
        &self,
        record: &CredentialRecord,
        refresh_fence: Option<[u8; 32]>,
        mut hook: impl FnMut(CommitPhase) -> Result<(), AuthError>,
    ) -> Result<(), AuthError> {
        record.data.validate()?;
        if record.data.refresh_uncertain.is_some() {
            return Err(AuthError::State);
        }
        self.check_host(record)?;
        let before = self.load()?;
        if before
            .as_ref()
            .is_some_and(|old| !old.data.same_registration(&record.data))
        {
            return Err(AuthError::Binding);
        }
        let registration = self.pending_registration()?;
        if registration
            .as_ref()
            .is_some_and(|registration| !registration.data.matches(&record.data))
        {
            return Err(AuthError::Binding);
        }
        let authorization = record
            .verified_authorization
            .swap(false, std::sync::atomic::Ordering::AcqRel);
        match refresh_fence {
            Some(fence) => {
                if before.as_ref().and_then(|old| old.data.refresh_uncertain) != Some(fence) {
                    return Err(AuthError::State);
                }
            }
            None => {
                if !authorization && before.as_ref().is_none_or(|old| old.data != record.data) {
                    return Err(AuthError::State);
                }
            }
        }
        // Attach before mutation, including failed saves, so this handle cannot
        // hand off credentials inconsistent with the selected vault afterwards.
        let source = self.record_source()?;
        *record.source.lock().map_err(|_| AuthError::Vault)? = Some(source);
        let bytes = serde_json::to_vec(&record.data).map_err(|_| AuthError::Vault)?;
        let pending = Staged::new(&self.directory, &bytes)?;
        hook(CommitPhase::BeforeReplace)?;
        // Recheck protected targets immediately before replacement.
        let after = self.load()?;
        if before.as_ref().map(|record| &record.data) != after.as_ref().map(|record| &record.data) {
            return Err(AuthError::State);
        }
        let current_registration = self.pending_registration()?;
        if registration.as_ref().map(|registration| &registration.data)
            != current_registration
                .as_ref()
                .map(|registration| &registration.data)
        {
            return Err(AuthError::State);
        }
        self.check_host(record)?;
        let recovery = read_recovery(&self.directory, &self.name)?;
        pending.replace(&self.name)?;
        hook(CommitPhase::AfterReplace)?;
        self.cleanup_registration(registration.as_ref())?;
        if let Some(recovery) = recovery {
            let actual = read_recovery(&self.directory, &self.name)?;
            if actual.as_ref() != Some(&recovery) {
                return Err(AuthError::State);
            }
            // The recovery snapshot remains authoritative through every fallible
            // commit operation, including a failed post-rename directory sync.
            // Removing it activates the entire already-durable replacement.
            remove_at(&self.directory, &suffixed_name(&self.name, REFRESH_SUFFIX)?)?;
            // No failure is reported after activation. If this cleanup sync
            // fails, a crash can restore the conservative fence, but cannot
            // restore an unfenced old credential: the replacement is durable.
            let _ = self.directory.sync_all();
        }
        Ok(())
    }

    /// Explicit refresh only. The account lock stays held through network I/O,
    /// signature and binding validation, and atomic replacement. No retry or
    /// fallback is performed. A remote rotation cannot be rolled back locally.
    pub async fn refresh(
        &mut self,
        transport: &dyn TokenTransport,
        verifier: &IdTokenVerifier,
        clock: &dyn Clock,
    ) -> Result<CredentialRecord, AuthError> {
        self.refresh_with_hook(transport, verifier, clock, |_| Ok(()))
            .await
    }

    async fn refresh_with_hook(
        &mut self,
        transport: &dyn TokenTransport,
        verifier: &IdTokenVerifier,
        clock: &dyn Clock,
        hook: impl FnMut(CommitPhase) -> Result<(), AuthError>,
    ) -> Result<CredentialRecord, AuthError> {
        let mut old = self.load()?.ok_or(AuthError::State)?;
        if old.data.refresh_uncertain.is_some() {
            return Err(AuthError::State);
        }
        let now = clock.now_unix();
        if now < old.data.saved_at_unix {
            return Err(AuthError::Expired);
        }
        if old.data.earliest_refresh_at.is_some_and(|at| now < at) {
            return Err(AuthError::TooEarly);
        }
        self.pending_registration()?;
        let mut fence = [0_u8; 32];
        getrandom::fill(&mut fence).map_err(|_| AuthError::Entropy)?;
        old.data.refresh_uncertain = Some(fence);
        let bytes = serde_json::to_vec(&old.data).map_err(|_| AuthError::Vault)?;
        let mut pending = Staged::new(&self.directory, &bytes)?;
        let recovery_name = suffixed_name(&self.name, REFRESH_SUFFIX)?;
        self.check_host(&old)?;
        // Publication includes file and directory fsync. No transport call, or
        // even construction of its future, precedes successful durable fencing.
        if !pending.publish_new(&recovery_name)? {
            return Err(AuthError::State);
        }
        let response = send_tokens(
            transport,
            form(&[
                ("grant_type", "refresh_token"),
                ("client_id", &old.data.client_id),
                ("refresh_token", &old.data.refresh_token),
                ("resource", RESOURCE),
            ]),
            TOKEN_TIMEOUT,
        )
        .await?;
        let selected = old.metadata();
        let replacement = finish_tokens(
            response,
            verifier,
            clock,
            GrantBinding {
                client_id: &old.data.client_id,
                host_id: &old.data.ext_agent_host_id,
                nonce: &old.data.nonce,
                selected: Some(&selected),
                refresh: true,
            },
        )?;
        self.save_with_hook(&replacement, Some(fence), hook)?;
        Ok(replacement)
    }
}

fn suffixed_name(name: &CStr, suffix: &str) -> Result<CString, AuthError> {
    let mut bytes = name.to_bytes().to_vec();
    bytes.extend_from_slice(suffix.as_bytes());
    CString::new(bytes).map_err(|_| AuthError::Vault)
}

fn read_credential(directory: &File, name: &CStr) -> Result<Option<StoredCredential>, AuthError> {
    let Some(file) = open_optional(directory, name)? else {
        return Ok(None);
    };
    let bytes = read_complete(file)?;
    let data: StoredCredential = serde_json::from_slice(&bytes).map_err(|_| AuthError::Vault)?;
    data.validate()?;
    Ok(Some(data))
}

fn read_recovery(directory: &File, name: &CStr) -> Result<Option<StoredCredential>, AuthError> {
    let recovery = read_credential(directory, &suffixed_name(name, REFRESH_SUFFIX)?)?;
    if recovery
        .as_ref()
        .is_some_and(|record| record.refresh_uncertain.is_none())
    {
        return Err(AuthError::Vault);
    }
    Ok(recovery)
}

fn read_effective(directory: &File, name: &CStr) -> Result<Option<StoredCredential>, AuthError> {
    let current = read_credential(directory, name)?;
    if current
        .as_ref()
        .is_some_and(|record| record.refresh_uncertain.is_some())
    {
        return Err(AuthError::Vault);
    }
    if let Some(recovery) = read_recovery(directory, name)? {
        let connected = current.as_ref().ok_or(AuthError::State)?;
        if !recovery.same_registration(connected) {
            return Err(AuthError::Binding);
        }
        // Keep the old credentials and registration available even if a new
        // file was renamed into place but its durable commit was not confirmed.
        return Ok(Some(recovery));
    }
    Ok(current)
}

fn read_pending_registration(
    directory: &File,
    name: &CStr,
) -> Result<Option<PendingRegistration>, AuthError> {
    let name = suffixed_name(name, REGISTRATION_SUFFIX)?;
    let Some(file) = open_optional(directory, &name)? else {
        return Ok(None);
    };
    let bytes = read_complete(file)?;
    let data: StoredRegistration = serde_json::from_slice(&bytes).map_err(|_| AuthError::Vault)?;
    data.validate()?;
    Ok(Some(PendingRegistration { data }))
}

fn remove_at(directory: &File, name: &CStr) -> Result<(), AuthError> {
    // SAFETY: the descriptor is live and the validated name is one component.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(AuthError::Vault);
    }
    Ok(())
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory-safety preconditions.
    unsafe { libc::geteuid() }
}

fn open_at(
    directory: &File,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<File> {
    // SAFETY: directory and NUL-terminated name remain alive for this call.
    let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, mode) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat transfers a fresh, uniquely owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_parent_checked(parent: &Path) -> Result<File, AuthError> {
    // SAFETY: the literal is NUL terminated; open returns a new descriptor.
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(AuthError::Vault);
    }
    // SAFETY: fd is fresh and ownership is transferred exactly once.
    let mut directory = unsafe { File::from_raw_fd(fd) };
    check_directory(&directory, false)?;
    for component in parent.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let name = CString::new(name.as_bytes()).map_err(|_| AuthError::Vault)?;
                let next = open_at(
                    &directory,
                    &name,
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    0,
                )
                .map_err(|_| AuthError::Vault)?;
                check_directory(&next, false)?;
                directory = next;
            }
            _ => return Err(AuthError::Vault),
        }
    }
    check_directory(&directory, true)?;
    Ok(directory)
}

fn check_directory(directory: &File, immediate: bool) -> Result<(), AuthError> {
    let metadata = directory.metadata().map_err(|_| AuthError::Vault)?;
    if !metadata.is_dir() {
        return Err(AuthError::Vault);
    }
    if immediate {
        if metadata.uid() != effective_uid() || metadata.mode() & 0o7777 != 0o700 {
            return Err(AuthError::Vault);
        }
    } else {
        // Descriptor-relative traversal avoids pathname re-resolution. A mapped
        // owner is allowed for ancestors only when group/other cannot write them;
        // that owner can still disrupt availability by renaming/chmodding paths.
        // The private leaf below remains strictly owned and mode 0700.
        if !ancestor_directory_permitted(metadata.uid(), metadata.mode()) {
            return Err(AuthError::Vault);
        }
    }
    Ok(())
}

fn ancestor_directory_permitted(owner_uid: u32, mode: u32) -> bool {
    mode & 0o022 == 0 || (owner_uid == 0 && mode & 0o1000 != 0)
}

fn check_file(file: &File) -> Result<Metadata, AuthError> {
    let metadata = file.metadata().map_err(|_| AuthError::Vault)?;
    if !metadata.is_file()
        || metadata.uid() != effective_uid()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(AuthError::Vault);
    }
    Ok(metadata)
}

fn open_optional(directory: &File, name: &CStr) -> Result<Option<File>, AuthError> {
    match open_at(directory, name, READ_FLAGS, 0) {
        Ok(file) => {
            check_file(&file)?;
            Ok(Some(file))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(AuthError::Vault),
    }
}

fn read_complete(mut file: File) -> Result<Vec<u8>, AuthError> {
    check_file(&file)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| AuthError::Vault)?;
    if check_file(&file)?.len() != bytes.len() as u64 {
        return Err(AuthError::Vault);
    }
    Ok(bytes)
}

fn read_host(directory: &File) -> Result<Option<String>, AuthError> {
    // The portable linkat publication can briefly have two links until its
    // staging name is removed. Wait for publication to finish without accepting
    // a multiply linked credential file or imposing a retry-count quota.
    loop {
        let file = match open_at(directory, HOST_FILE, READ_FLAGS, 0) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(AuthError::Vault),
        };
        let metadata = file.metadata().map_err(|_| AuthError::Vault)?;
        if !metadata.is_file()
            || metadata.uid() != effective_uid()
            || metadata.mode() & 0o7777 != 0o600
        {
            return Err(AuthError::Vault);
        }
        if metadata.nlink() == 2 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
        let bytes = read_complete(file)?;
        let host = String::from_utf8(bytes).map_err(|_| AuthError::Vault)?;
        if !valid_host_id(&host) {
            return Err(AuthError::Vault);
        }
        return Ok(Some(host));
    }
}

struct Staged<'a> {
    directory: &'a File,
    name: CString,
    present: bool,
}

impl<'a> Staged<'a> {
    fn new(directory: &'a File, bytes: &[u8]) -> Result<Self, AuthError> {
        Self::stage(directory, bytes)
    }

    fn stage(directory: &'a File, bytes: &[u8]) -> Result<Self, AuthError> {
        loop {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random).map_err(|_| AuthError::Entropy)?;
            let name = CString::new(format!("{TEMP_PREFIX}{}", hex::encode(random)))
                .map_err(|_| AuthError::Vault)?;
            let mut file = match open_at(
                directory,
                &name,
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK,
                0o600,
            ) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(AuthError::Vault),
            };
            let pending = Self {
                directory,
                name,
                present: true,
            };
            check_file(&file)?;
            file.write_all(bytes).map_err(|_| AuthError::Vault)?;
            file.sync_all().map_err(|_| AuthError::Vault)?;
            return Ok(pending);
        }
    }

    fn replace(mut self, target: &CStr) -> Result<(), AuthError> {
        // SAFETY: both names are single components and both descriptors are live.
        if unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
                self.directory.as_raw_fd(),
                target.as_ptr(),
            )
        } != 0
        {
            return Err(AuthError::Vault);
        }
        self.present = false;
        self.directory
            .sync_all()
            .map_err(|_| AuthError::CommitUncertain)
    }

    fn publish_new(&mut self, target: &CStr) -> Result<bool, AuthError> {
        // Linux provides atomic no-replace rename without a transient hard link.
        // Unsupported filesystems fail closed. Other Unix platforms use linkat.
        #[cfg(target_os = "linux")]
        // SAFETY: live descriptors and NUL-terminated single-component names.
        let result = unsafe {
            libc::renameat2(
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
                self.directory.as_raw_fd(),
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(not(target_os = "linux"))]
        // SAFETY: live descriptors and NUL-terminated single-component names.
        let result = unsafe {
            libc::linkat(
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
                self.directory.as_raw_fd(),
                target.as_ptr(),
                0,
            )
        };
        if result != 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::AlreadyExists {
                return Ok(false);
            }
            return Err(AuthError::Vault);
        }
        #[cfg(not(target_os = "linux"))]
        {
            // SAFETY: removes only this staging name from the anchored directory.
            if unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) } != 0 {
                return Err(AuthError::CommitUncertain);
            }
        }
        self.present = false;
        self.directory
            .sync_all()
            .map_err(|_| AuthError::CommitUncertain)?;
        Ok(true)
    }
}

impl Drop for Staged<'_> {
    fn drop(&mut self) {
        if self.present {
            // SAFETY: removes only our randomly generated staging component.
            let _ = unsafe { libc::unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0) };
        }
    }
}
