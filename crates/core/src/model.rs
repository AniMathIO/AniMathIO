use crate::{Error, FrameRate, Keyframes, Time, invalid};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Stable clip identity, allocated by the command caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClipId(pub u64);
/// Stable track identity, allocated by the command caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrackId(pub u64);

/// Lowercase BLAKE3 content hash, serialized as 64 hex characters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AssetId(String);
impl AssetId {
    /// Hash asset bytes without opening or retaining any resource.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }
    /// Canonical hash used as the archive entry name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for AssetId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid("asset id must be a lowercase BLAKE3 hash"));
        }
        Ok(Self(value))
    }
}
impl From<AssetId> for String {
    fn from(value: AssetId) -> Self {
        value.0
    }
}

/// Resource-free asset metadata. Bytes live in the format/engine layers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// Human-readable filename.
    pub name: String,
    /// Declared media type; decoders must probe the actual bytes.
    pub media_type: String,
}

/// Half-open nonnegative interval [start, end), measured in timeline ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    /// Inclusive start.
    pub start: Time,
    /// Exclusive end.
    pub end: Time,
}
impl TimeRange {
    /// Construct a nonempty, nonnegative range.
    pub fn new(start: Time, end: Time) -> Result<Self, Error> {
        let range = Self { start, end };
        range.validate()?;
        Ok(range)
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        if self.start.0 < 0 || self.end <= self.start {
            return Err(invalid("invalid time range"));
        }
        Ok(())
    }
}

/// Resolution-independent transform. Position and scale are fractions of the canvas;
/// off-canvas positions and negative scale (mirroring) are allowed. Rotation is radians.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Horizontal canvas fraction.
    pub x: f64,
    /// Vertical canvas fraction.
    pub y: f64,
    /// Horizontal scale relative to canvas width.
    pub scale_x: f64,
    /// Vertical scale relative to canvas height.
    pub scale_y: f64,
    /// Rotation in radians.
    pub rotation: f64,
    /// Opacity in [0, 1].
    pub opacity: f64,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            opacity: 1.0,
        }
    }
}
impl Transform {
    fn valid(self) -> bool {
        [
            self.x,
            self.y,
            self.scale_x,
            self.scale_y,
            self.rotation,
            self.opacity,
        ]
        .iter()
        .all(|x| x.is_finite())
            && (0.0..=1.0).contains(&self.opacity)
    }
    pub(crate) fn set(&mut self, property: TransformProperty, value: f64) {
        *match property {
            TransformProperty::X => &mut self.x,
            TransformProperty::Y => &mut self.y,
            TransformProperty::ScaleX => &mut self.scale_x,
            TransformProperty::ScaleY => &mut self.scale_y,
            TransformProperty::Rotation => &mut self.rotation,
            TransformProperty::Opacity => &mut self.opacity,
        } = value;
    }
}

/// Core media or an opaque plugin source, preserved even without that plugin installed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClipKind {
    /// Decoded video source.
    Video(AssetId),
    /// Still image source.
    Image(AssetId),
    /// Audio source, included in the scene for a future audio mixer.
    Audio(AssetId),
    /// Namespaced plugin identity and opaque, plugin-versioned JSON data.
    Plugin {
        /// Reverse-domain plugin identifier.
        plugin_id: String,
        /// Preserved verbatim; numeric automation lives in `Clip::params`.
        params: serde_json::Value,
    },
    /// Resolved vector scene, e.g. produced by a plugin or vector clip source.
    Vector(crate::VectorScene),
    /// Errored clip, containing an error message.
    Error(String),
}

/// A transform component that can be animated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TransformProperty {
    /// Horizontal position.
    X,
    /// Vertical position.
    Y,
    /// Horizontal scale.
    ScaleX,
    /// Vertical scale.
    ScaleY,
    /// Rotation in radians.
    Rotation,
    /// Opacity.
    Opacity,
}
/// Typed numeric animation target.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    /// Transform component.
    Transform(TransformProperty),
    /// Named numeric parameter; must exist in the clip's base parameters.
    Param(String),
}
/// One numeric animation in clip-local ticks (keyframes may extend beyond a trim).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// Property to override.
    pub target: Target,
    /// Ordered keyframes.
    pub keys: Keyframes<f64>,
}

/// A draft clip. Once inserted, only commands can change the project's copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Stable identity, unique across the project.
    pub id: ClipId,
    /// Source data.
    pub kind: ClipKind,
    /// Timeline placement.
    pub range: TimeRange,
    /// Source offset at the clip's start (also used by plugins).
    pub source_offset: Time,
    /// Base transform.
    pub transform: Transform,
    /// Named base numeric parameters (e.g. gain or a graph coefficient).
    pub params: BTreeMap<String, f64>,
    /// Numeric curves in clip-local time, sorted by unique target.
    pub animations: Vec<Animation>,
}
impl Clip {
    /// Construct a draft clip with an identity transform and no automation.
    #[must_use]
    pub fn new(id: ClipId, kind: ClipKind, range: TimeRange) -> Self {
        Self {
            id,
            kind,
            range,
            source_offset: Time(0),
            transform: Transform::default(),
            params: BTreeMap::new(),
            animations: Vec::new(),
        }
    }
}

/// Ordered compositing layer. Read access never exposes mutable project state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub(crate) id: TrackId,
    pub(crate) name: String,
    pub(crate) clips: BTreeMap<ClipId, Clip>,
    #[serde(default = "unity_gain")]
    pub(crate) gain: f64,
}
impl Track {
    /// Construct an empty layer.
    #[must_use]
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            clips: BTreeMap::new(),
            gain: 1.0,
        }
    }
    /// Linear audio gain (unity by default).
    #[must_use]
    pub fn gain(&self) -> f64 {
        self.gain
    }
    /// Stable layer identity.
    #[must_use]
    pub fn id(&self) -> TrackId {
        self.id
    }
    /// Layer label.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Clips on this layer.
    #[must_use]
    pub fn clips(&self) -> &BTreeMap<ClipId, Clip> {
        &self.clips
    }
}

/// Serializable, resource-free project. Mutations go through `Command::apply`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ProjectData", into = "ProjectData")]
pub struct Project {
    pub(crate) frame_rate: FrameRate,
    pub(crate) tracks: Vec<Track>,
    pub(crate) assets: BTreeMap<AssetId, Asset>,
}
#[derive(Serialize, Deserialize)]
struct ProjectData {
    frame_rate: FrameRate,
    tracks: Vec<Track>,
    assets: BTreeMap<AssetId, Asset>,
}
impl From<Project> for ProjectData {
    fn from(p: Project) -> Self {
        Self {
            frame_rate: p.frame_rate,
            tracks: p.tracks,
            assets: p.assets,
        }
    }
}
impl TryFrom<ProjectData> for Project {
    type Error = Error;
    fn try_from(p: ProjectData) -> Result<Self, Error> {
        let project = Self {
            frame_rate: p.frame_rate,
            tracks: p.tracks,
            assets: p.assets,
        };
        project.validate()?;
        Ok(project)
    }
}
impl Project {
    /// Empty project at the requested frame rate.
    #[must_use]
    pub fn new(frame_rate: FrameRate) -> Self {
        Self {
            frame_rate,
            ..Self::default()
        }
    }
    /// Output frame rate.
    #[must_use]
    pub fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }
    /// Layers in back-to-front compositing order.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
    /// Content-addressed asset metadata.
    #[must_use]
    pub fn assets(&self) -> &BTreeMap<AssetId, Asset> {
        &self.assets
    }
    /// Check identities, references, finite values and nonoverlapping half-open ranges.
    pub fn validate(&self) -> Result<(), Error> {
        let mut track_ids = BTreeSet::new();
        let mut clip_ids = BTreeSet::new();
        for track in &self.tracks {
            if !track_ids.insert(track.id) {
                return Err(invalid("duplicate track id"));
            }
            if !track.gain.is_finite() || track.gain < 0.0 {
                return Err(invalid("invalid track gain"));
            }
            let mut ranges = Vec::new();
            for (id, clip) in &track.clips {
                if id != &clip.id || !clip_ids.insert(*id) {
                    return Err(invalid("duplicate or mismatched clip id"));
                }
                clip.range.validate()?;
                if clip.source_offset.0 < 0
                    || clip
                        .source_offset
                        .0
                        .checked_add(clip.range.end.0 - clip.range.start.0)
                        .is_none()
                {
                    return Err(invalid("invalid source time range"));
                }
                if !clip.transform.valid() || clip.params.values().any(|v| !v.is_finite()) {
                    return Err(invalid("invalid transform or parameter"));
                }
                match &clip.kind {
                    ClipKind::Video(id) | ClipKind::Image(id) | ClipKind::Audio(id) => {
                        if !self.assets.contains_key(id) {
                            return Err(invalid("missing asset"));
                        }
                    }
                    ClipKind::Plugin { plugin_id, .. } if plugin_id.is_empty() => {
                        return Err(invalid("empty plugin id"));
                    }
                    ClipKind::Plugin { .. } | ClipKind::Error(_) => {}
                    ClipKind::Vector(scene) => scene.validate()?,
                }
                if clip
                    .animations
                    .windows(2)
                    .any(|pair| pair[0].target >= pair[1].target)
                {
                    return Err(invalid("animations must be sorted by target"));
                }
                for animation in &clip.animations {
                    if animation.keys.0.is_empty() {
                        return Err(invalid("empty animation"));
                    }
                    if let Target::Param(name) = &animation.target
                        && !clip.params.contains_key(name)
                    {
                        return Err(invalid("animation parameter missing"));
                    }
                    for (time, key) in &animation.keys.0 {
                        if time.0 < 0
                            || !key.value.is_finite()
                            || (animation.target == Target::Transform(TransformProperty::Opacity)
                                && !(0.0..=1.0).contains(&key.value))
                        {
                            return Err(invalid("invalid keyframe"));
                        }
                    }
                }
                ranges.push(clip.range);
            }
            ranges.sort_by_key(|r| r.start);
            if ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
                return Err(invalid("overlapping clips"));
            }
        }
        Ok(())
    }
}

/// Resolved input to future visual renderers and the audio mixer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// Active clips in back-to-front track order.
    pub clips: Vec<SceneClip>,
}
/// One resolved active clip, containing no runtime resources.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneClip {
    /// Layer identity.
    pub track: TrackId,
    /// Clip identity.
    pub id: ClipId,
    /// Source description, including preserved plugin data.
    pub kind: ClipKind,
    /// Source-local tick position.
    pub source_time: Time,
    /// Evaluated transform.
    pub transform: Transform,
    /// Evaluated numeric parameters.
    pub params: BTreeMap<String, f64>,
}

/// Resolve active clips at `time`, independently of previous calls or seek order.
/// Ranges are half-open; audio sources are included for the future mixer.
#[must_use]
pub fn evaluate(project: &Project, time: Time) -> Scene {
    let mut scene = Scene::default();
    for track in &project.tracks {
        for clip in track.clips.values() {
            if time < clip.range.start || time >= clip.range.end {
                continue;
            }
            let local_time = Time(time.0 - clip.range.start.0);
            let mut resolved = SceneClip {
                track: track.id,
                id: clip.id,
                kind: clip.kind.clone(),
                source_time: Time(clip.source_offset.0 + local_time.0),
                transform: clip.transform,
                params: clip.params.clone(),
            };
            for animation in &clip.animations {
                if let Some(value) = animation.keys.sample(local_time) {
                    match &animation.target {
                        Target::Transform(property) => resolved.transform.set(*property, value),
                        Target::Param(name) => {
                            resolved.params.insert(name.clone(), value);
                        }
                    }
                }
            }
            scene.clips.push(resolved);
        }
    }
    scene
}

fn unity_gain() -> f64 {
    1.0
}
