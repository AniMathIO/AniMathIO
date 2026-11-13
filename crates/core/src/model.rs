use crate::builtin::Color;
use crate::{EffectId, EffectInstance, Error, FrameRate, Keyframes, ParamValue, Time, invalid};
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
    /// Temporary text/shape sources; replaced by plugins after the prototype.
    Builtin(crate::builtin::Builtin),
    /// Namespaced plugin identity and opaque, plugin-versioned JSON data.
    Plugin {
        /// Reverse-domain plugin identifier.
        plugin_id: String,
        /// Preserved verbatim; typed automation lives in `Clip::params`.
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
/// Typed animation target.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    /// Transform component.
    Transform(TransformProperty),
    /// Named parameter; plugin curves may override an implicit schema default.
    Param(String),
    /// Parameter of one effect instance, addressed by its stable effect id.
    Effect {
        /// Effect identity within the clip.
        effect: EffectId,
        /// Parameter name within that effect.
        param: String,
    },
}
/// One typed animation in clip-local ticks (keyframes may extend beyond a trim).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// Property to override.
    pub target: Target,
    /// Ordered keyframes.
    pub keys: Keyframes<ParamValue>,
}

/// A draft clip. Once inserted, only commands can change the project's copy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Stable identity, unique across the project.
    pub id: ClipId,
    /// Clips with the same group move and edit together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_group: Option<ClipId>,
    /// Source data.
    pub kind: ClipKind,
    /// Timeline placement.
    pub range: TimeRange,
    /// Source offset at the clip's start (also used by plugins).
    pub source_offset: Time,
    /// Base transform.
    pub transform: Transform,
    /// Named base typed parameters (e.g. gain or a graph coefficient).
    pub params: BTreeMap<String, ParamValue>,
    /// Ordered visual effects, applied after rasterizing the clip source.
    #[serde(default)]
    pub effects: Vec<EffectInstance>,
    /// Typed curves in clip-local time, sorted by unique target.
    pub animations: Vec<Animation>,
}
impl Clip {
    /// Construct a draft clip with an identity transform and no automation.
    #[must_use]
    pub fn new(id: ClipId, kind: ClipKind, range: TimeRange) -> Self {
        Self {
            id,
            link_group: None,
            kind,
            range,
            source_offset: Time(0),
            transform: Transform::default(),
            params: BTreeMap::new(),
            effects: Vec::new(),
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
    /// Muted layers are silent in the mix and hidden from rendering.
    #[serde(default)]
    pub(crate) muted: bool,
    /// Soloed layers are the only ones audible/visible while any layer is soloed.
    #[serde(default)]
    pub(crate) solo: bool,
    /// Locked layers reject clip edits (add, move, trim, split, delete, effects).
    #[serde(default)]
    pub(crate) locked: bool,
    /// Edit-point transitions between adjacent clips of this track.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) transitions: Vec<crate::Transition>,
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
            muted: false,
            solo: false,
            locked: false,
            transitions: Vec::new(),
        }
    }
    /// Linear audio gain (unity by default).
    #[must_use]
    pub fn gain(&self) -> f64 {
        self.gain
    }
    /// Whether the layer is muted.
    #[must_use]
    pub const fn is_muted(&self) -> bool {
        self.muted
    }
    /// Whether the layer is soloed.
    #[must_use]
    pub const fn is_solo(&self) -> bool {
        self.solo
    }
    /// Whether the layer rejects clip edits.
    #[must_use]
    pub const fn is_locked(&self) -> bool {
        self.locked
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
/// A resolution of `(0, 0)` means the project has no explicit output size yet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ProjectData", into = "ProjectData")]
pub struct Project {
    pub(crate) frame_rate: FrameRate,
    pub(crate) tracks: Vec<Track>,
    pub(crate) assets: BTreeMap<AssetId, Asset>,
    /// Output width in pixels, or `0` when unset.
    pub(crate) width: u32,
    /// Output height in pixels, or `0` when unset.
    pub(crate) height: u32,
    /// Straight-alpha sRGB canvas background; alpha `0` is transparent.
    pub(crate) background: Color,
    /// Explicit project duration in ticks; `0` means derive from clips.
    pub(crate) duration: Time,
}
#[derive(Serialize, Deserialize)]
struct ProjectData {
    frame_rate: FrameRate,
    tracks: Vec<Track>,
    assets: BTreeMap<AssetId, Asset>,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    background: Color,
    #[serde(default)]
    duration: Time,
}
impl From<Project> for ProjectData {
    fn from(p: Project) -> Self {
        Self {
            frame_rate: p.frame_rate,
            tracks: p.tracks,
            assets: p.assets,
            width: p.width,
            height: p.height,
            background: p.background,
            duration: p.duration,
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
            width: p.width,
            height: p.height,
            background: p.background,
            duration: p.duration,
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
    /// Output width in pixels; `0` means no explicit resolution is set.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }
    /// Output height in pixels; `0` means no explicit resolution is set.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }
    /// Straight-alpha sRGB canvas background.
    #[must_use]
    pub const fn background(&self) -> Color {
        self.background
    }
    /// Explicit project duration in ticks; `0` means derive from clips.
    #[must_use]
    pub const fn duration(&self) -> Time {
        self.duration
    }
    /// Check identities, references, finite values and nonoverlapping half-open ranges.
    pub fn validate(&self) -> Result<(), Error> {
        if self.width > 65_535 || self.height > 65_535 {
            return Err(invalid("invalid resolution"));
        }
        if self.width != 0
            && self.height != 0
            && u64::from(self.width) * u64::from(self.height) > 32 * 1024 * 1024
        {
            return Err(invalid("resolution exceeds 32 megapixels"));
        }
        if self.duration.0 < 0 {
            return Err(invalid("negative project duration"));
        }
        let mut track_ids = BTreeSet::new();
        let mut clip_ids = BTreeSet::new();
        let mut transition_ids = BTreeSet::new();
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
                if !clip.transform.valid() || clip.params.values().any(|v| !v.valid()) {
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
                    ClipKind::Builtin(source) => source.validate()?,
                    ClipKind::Vector(scene) => scene.validate()?,
                }
                validate_clip_effects_and_animations(clip)?;
                ranges.push(clip.range);
            }
            ranges.sort_by_key(|r| r.start);
            if ranges.windows(2).any(|pair| pair[0].end > pair[1].start) {
                return Err(invalid("overlapping clips"));
            }
            track.validate_transitions()?;
            if !track
                .transitions
                .iter()
                .all(|t| transition_ids.insert(t.id))
            {
                return Err(invalid("duplicate transition id"));
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
    /// Evaluated typed parameters.
    pub params: BTreeMap<String, ParamValue>,
    /// Evaluated effect chain, with keyframed parameters resolved.
    #[serde(default)]
    pub effects: Vec<EffectInstance>,
    /// Transition state when this clip is inside a transition window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<crate::SceneTransition>,
}

/// Resolve active clips at `time`, independently of previous calls or seek order.
/// Ranges are half-open; audio sources are included for the future mixer.
/// Inside a transition window both the outgoing and the incoming clip are active and
/// carry the same [`crate::SceneTransition`]. The incoming clip's local time is then
/// negative; its source time is clamped to zero (holding the first source frame).
#[must_use]
pub fn evaluate(project: &Project, time: Time) -> Scene {
    let mut scene = Scene::default();
    let any_solo = project.tracks.iter().any(Track::is_solo);
    for track in &project.tracks {
        if track.muted || (any_solo && !track.solo) {
            continue;
        }
        for clip in track.clips.values() {
            let span = track.clip_span(clip);
            if time < span.range.start || time >= span.range.end {
                continue;
            }
            let local_time = Time(time.0 - clip.range.start.0);
            let mut resolved = SceneClip {
                track: track.id,
                id: clip.id,
                kind: clip.kind.clone(),
                source_time: Time((clip.source_offset.0 + local_time.0).max(0)),
                transform: clip.transform,
                params: clip.params.clone(),
                effects: clip.effects.clone(),
                transition: span.transition_at(clip, time),
            };
            for animation in &clip.animations {
                if let Some(value) = animation.keys.sample(local_time) {
                    match &animation.target {
                        Target::Transform(property) => {
                            if let Some(number) = value.as_number() {
                                resolved.transform.set(*property, number);
                            }
                        }
                        Target::Param(name) => {
                            resolved.params.insert(name.clone(), value);
                        }
                        Target::Effect { effect, param } => {
                            if let Some(instance) = resolved
                                .effects
                                .iter_mut()
                                .find(|instance| instance.id == *effect)
                                && let Some(number) = value.as_number()
                            {
                                instance.params.insert(param.clone(), number);
                            }
                        }
                    }
                }
            }
            scene.clips.push(resolved);
        }
    }
    scene
}

fn validate_clip_effects_and_animations(clip: &Clip) -> Result<(), Error> {
    let mut effect_ids = BTreeSet::new();
    for effect in &clip.effects {
        if !effect_ids.insert(effect.id) {
            return Err(invalid("duplicate effect id"));
        }
        effect.validate()?;
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
        let effect_kind = match &animation.target {
            Target::Param(name) => {
                if !clip.params.contains_key(name) && !matches!(&clip.kind, ClipKind::Plugin { .. })
                {
                    return Err(invalid("animation parameter missing"));
                }
                None
            }
            Target::Effect { effect, param } => {
                let instance = clip
                    .effects
                    .iter()
                    .find(|instance| instance.id == *effect)
                    .ok_or_else(|| invalid("animation effect missing"))?;
                if !instance.kind.param_names().contains(&param.as_str()) {
                    return Err(invalid("animation effect parameter missing"));
                }
                Some(instance.kind)
            }
            Target::Transform(_) => None,
        };
        let first = &animation
            .keys
            .0
            .first_key_value()
            .expect("nonempty animation")
            .1
            .value;
        if let Target::Param(name) = &animation.target
            && let Some(base) = clip.params.get(name)
            && !base.same_type(first)
        {
            return Err(invalid("keyframe type differs from base parameter"));
        }
        for (time, key) in &animation.keys.0 {
            let valid_value = match &animation.target {
                Target::Transform(TransformProperty::Opacity) => key
                    .value
                    .as_number()
                    .is_some_and(|v| (0.0..=1.0).contains(&v)),
                Target::Transform(_) => key.value.as_number().is_some(),
                Target::Param(_) => true,
                Target::Effect { param, .. } => effect_kind.is_some_and(|kind| {
                    key.value
                        .as_number()
                        .is_some_and(|v| kind.validate_param(param, v).is_ok())
                }),
            };
            if time.0 < 0 || !key.value.valid() || !key.value.same_type(first) || !valid_value {
                return Err(invalid("invalid or mixed-type keyframe"));
            }
        }
    }
    Ok(())
}

fn unity_gain() -> f64 {
    1.0
}
