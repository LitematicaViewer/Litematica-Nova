use std::cmp::Ordering;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

const DEFAULT_NAMESPACE: &str = "minecraft";

/// An exact Minecraft logical identifier.
///
/// Unlike [`ResourceKey`], this type intentionally preserves empty paths,
/// repeated separators, and `.`/`..` path segments because MC 26.2's
/// `Identifier` validates characters without normalizing the path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MinecraftIdentifier {
    namespace: Box<str>,
    path: Box<str>,
}

impl MinecraftIdentifier {
    pub fn from_parts(namespace: &str, path: &str) -> Result<Self, MinecraftIdentifierError> {
        validate_identifier_namespace(namespace)?;
        validate_identifier_path(path)?;
        Ok(Self {
            namespace: namespace.into(),
            path: path.into(),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn with_path(&self, path: &str) -> Result<Self, MinecraftIdentifierError> {
        Self::from_parts(self.namespace(), path)
    }

    pub fn with_prefix(&self, prefix: &str) -> Result<Self, MinecraftIdentifierError> {
        self.with_path(&format!("{prefix}{}", self.path()))
    }

    pub fn with_suffix(&self, suffix: &str) -> Result<Self, MinecraftIdentifierError> {
        self.with_path(&format!("{}{suffix}", self.path()))
    }
}

impl FromStr for MinecraftIdentifier {
    type Err = MinecraftIdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.split_once(':') {
            Some(("", path)) => Self::from_parts(DEFAULT_NAMESPACE, path),
            Some((namespace, path)) => Self::from_parts(namespace, path),
            None => Self::from_parts(DEFAULT_NAMESPACE, value),
        }
    }
}

impl Display for MinecraftIdentifier {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.namespace, self.path)
    }
}

impl PartialOrd for MinecraftIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MinecraftIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.namespace.cmp(&other.namespace))
    }
}

impl From<&ResourceKey> for MinecraftIdentifier {
    fn from(value: &ResourceKey) -> Self {
        Self {
            namespace: value.namespace.clone(),
            path: value.path.clone(),
        }
    }
}

impl From<ResourceKey> for MinecraftIdentifier {
    fn from(value: ResourceKey) -> Self {
        Self {
            namespace: value.namespace,
            path: value.path,
        }
    }
}

impl TryFrom<&MinecraftIdentifier> for ResourceKey {
    type Error = ResourceKeyError;

    fn try_from(value: &MinecraftIdentifier) -> Result<Self, Self::Error> {
        Self::from_parts(value.namespace(), value.path())
    }
}

impl TryFrom<MinecraftIdentifier> for ResourceKey {
    type Error = ResourceKeyError;

    fn try_from(value: MinecraftIdentifier) -> Result<Self, Self::Error> {
        Self::from_parts(&value.namespace, &value.path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MinecraftIdentifierError {
    InvalidNamespaceCharacter { index: usize, character: char },
    InvalidPathCharacter { index: usize, character: char },
    UnsafeNamespace,
}

impl Display for MinecraftIdentifierError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidNamespaceCharacter { index, character } => write!(
                formatter,
                "invalid Minecraft identifier namespace character {character:?} at byte {index}"
            ),
            Self::InvalidPathCharacter { index, character } => write!(
                formatter,
                "invalid Minecraft identifier path character {character:?} at byte {index}"
            ),
            Self::UnsafeNamespace => {
                formatter.write_str("Minecraft identifier namespace must not be '..'")
            }
        }
    }
}

impl Error for MinecraftIdentifierError {}

fn validate_identifier_namespace(namespace: &str) -> Result<(), MinecraftIdentifierError> {
    if namespace == ".." {
        return Err(MinecraftIdentifierError::UnsafeNamespace);
    }
    for (index, character) in namespace.char_indices() {
        if !(character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-' | '.'))
        {
            return Err(MinecraftIdentifierError::InvalidNamespaceCharacter { index, character });
        }
    }
    Ok(())
}

fn validate_identifier_path(path: &str) -> Result<(), MinecraftIdentifierError> {
    for (index, character) in path.char_indices() {
        if !(character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-' | '.' | '/'))
        {
            return Err(MinecraftIdentifierError::InvalidPathCharacter { index, character });
        }
    }
    Ok(())
}

/// A canonical, path-safe Minecraft resource identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    namespace: Box<str>,
    path: Box<str>,
}

impl ResourceKey {
    pub fn from_parts(namespace: &str, path: &str) -> Result<Self, ResourceKeyError> {
        validate_namespace(namespace)?;
        validate_path(path)?;
        Ok(Self {
            namespace: namespace.into(),
            path: path.into(),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl FromStr for ResourceKey {
    type Err = ResourceKeyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.split_once(':') {
            Some(("", path)) => Self::from_parts(DEFAULT_NAMESPACE, path),
            Some((namespace, path)) => Self::from_parts(namespace, path),
            None => Self::from_parts(DEFAULT_NAMESPACE, value),
        }
    }
}

impl Display for ResourceKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.namespace, self.path)
    }
}

impl PartialOrd for ResourceKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ResourceKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.namespace.cmp(&other.namespace))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceKeyError {
    EmptyNamespace,
    EmptyPath,
    InvalidNamespaceCharacter { index: usize, character: char },
    InvalidPathCharacter { index: usize, character: char },
    UnsafeNamespace,
    EmptyPathSegment { index: usize },
    UnsafePathSegment { index: usize },
}

impl Display for ResourceKeyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyNamespace => formatter.write_str("resource namespace must not be empty"),
            Self::EmptyPath => formatter.write_str("resource path must not be empty"),
            Self::InvalidNamespaceCharacter { index, character } => write!(
                formatter,
                "invalid resource namespace character {character:?} at byte {index}"
            ),
            Self::InvalidPathCharacter { index, character } => write!(
                formatter,
                "invalid resource path character {character:?} at byte {index}"
            ),
            Self::UnsafeNamespace => formatter.write_str("resource namespace must not be '..'"),
            Self::EmptyPathSegment { index } => {
                write!(formatter, "resource path segment {index} must not be empty")
            }
            Self::UnsafePathSegment { index } => write!(
                formatter,
                "resource path segment {index} must not be '.' or '..'"
            ),
        }
    }
}

impl Error for ResourceKeyError {}

fn validate_namespace(namespace: &str) -> Result<(), ResourceKeyError> {
    if namespace.is_empty() {
        return Err(ResourceKeyError::EmptyNamespace);
    }
    if namespace == ".." {
        return Err(ResourceKeyError::UnsafeNamespace);
    }
    for (index, character) in namespace.char_indices() {
        if !(character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-' | '.'))
        {
            return Err(ResourceKeyError::InvalidNamespaceCharacter { index, character });
        }
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), ResourceKeyError> {
    if path.is_empty() {
        return Err(ResourceKeyError::EmptyPath);
    }
    for (index, character) in path.char_indices() {
        if !(character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-' | '.' | '/'))
        {
            return Err(ResourceKeyError::InvalidPathCharacter { index, character });
        }
    }
    for (index, segment) in path.split('/').enumerate() {
        if segment.is_empty() {
            return Err(ResourceKeyError::EmptyPathSegment { index });
        }
        if matches!(segment, "." | "..") {
            return Err(ResourceKeyError::UnsafePathSegment { index });
        }
    }
    Ok(())
}

/// Monotonic identity of an immutable resource snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceGeneration(u64);

impl ResourceGeneration {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// SHA-256 identity of the resolved resource bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceContentHash([u8; 32]);

impl ResourceContentHash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Canonical two-component Minecraft release identity (for example `26.2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MinecraftRelease {
    major: u16,
    minor: u16,
}

impl MinecraftRelease {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn major(self) -> u16 {
        self.major
    }

    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// Release and data-version pair that controls Minecraft resource interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MinecraftSemanticVersion {
    release: MinecraftRelease,
    data_version: u32,
}

impl MinecraftSemanticVersion {
    pub const fn new(release: MinecraftRelease, data_version: u32) -> Self {
        Self {
            release,
            data_version,
        }
    }

    pub const fn release(self) -> MinecraftRelease {
        self.release
    }

    pub const fn data_version(self) -> u32 {
        self.data_version
    }
}

/// Complete CPU cache identity. A value from one snapshot cannot hit another generation's cache.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceCacheKey<P> {
    resource: ResourceKey,
    generation: ResourceGeneration,
    content_hash: ResourceContentHash,
    minecraft_semantics: MinecraftSemanticVersion,
    bake_parameters: P,
}

impl<P> ResourceCacheKey<P> {
    pub fn new(
        resource: ResourceKey,
        generation: ResourceGeneration,
        content_hash: ResourceContentHash,
        minecraft_semantics: MinecraftSemanticVersion,
        bake_parameters: P,
    ) -> Self {
        Self {
            resource,
            generation,
            content_hash,
            minecraft_semantics,
            bake_parameters,
        }
    }

    pub fn resource(&self) -> &ResourceKey {
        &self.resource
    }

    pub fn generation(&self) -> ResourceGeneration {
        self.generation
    }

    pub fn content_hash(&self) -> ResourceContentHash {
        self.content_hash
    }

    pub fn minecraft_semantics(&self) -> MinecraftSemanticVersion {
        self.minecraft_semantics
    }

    pub fn bake_parameters(&self) -> &P {
        &self.bake_parameters
    }
}
