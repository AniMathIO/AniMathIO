use serde::{Deserialize, Serialize};

/// Straight-alpha RGBA color value for parameter defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorValue {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel.
    pub a: u8,
}

/// 2D point value for parameter defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PointValue {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
}

/// Slider control metadata for continuous numeric values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SliderControl {
    /// Minimum value.
    pub min: f64,
    /// Maximum value.
    pub max: f64,
    /// Default value.
    pub default: f64,
    /// Increment step.
    pub step: f64,
}

/// UI widget representation for parameter inspector controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum ControlWidget {
    /// Numeric slider and spinbox.
    Slider(SliderControl),
    /// RGBA color picker.
    ColorPicker {
        /// Default color.
        default: ColorValue,
    },
    /// Text input box.
    TextInput {
        /// Default string.
        default: String,
    },
    /// Dropdown selector with discrete choices.
    Dropdown {
        /// Available string options.
        options: Vec<String>,
        /// Selected default.
        default: String,
    },
    /// Checkbox toggle.
    Toggle {
        /// Default boolean state.
        default: bool,
    },
    /// 2D coordinate picker or canvas handle.
    Point2D {
        /// Default coordinates.
        default: PointValue,
    },
    /// Media asset selector.
    AssetPicker {
        /// Filter by media type, e.g. "image/*", "video/*".
        media_type: String,
        /// Default asset id if any.
        default_asset: Option<String>,
    },
}

/// One inspectable parameter in the editor UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InspectorParam {
    /// Parameter identifier.
    pub id: String,
    /// Human-friendly label.
    pub label: String,
    /// Whether this parameter can be animated on the timeline with keyframes.
    pub keyframeable: bool,
    /// UI control configuration.
    pub widget: ControlWidget,
}

/// Schema representing all editable parameters of a plugin, consumed by the UI.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InspectorSchema {
    /// Ordered list of controls for inspector panels.
    pub params: Vec<InspectorParam>,
}

impl InspectorSchema {
    /// Create an empty schema.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a slider parameter.
    #[must_use]
    pub fn with_number(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        min: f64,
        max: f64,
        default: f64,
        step: f64,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::Slider(SliderControl {
                min,
                max,
                default,
                step,
            }),
        });
        self
    }

    /// Add a color parameter.
    #[must_use]
    pub fn with_color(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        default: ColorValue,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::ColorPicker { default },
        });
        self
    }

    /// Add a string parameter.
    #[must_use]
    pub fn with_string(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        default: impl Into<String>,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::TextInput {
                default: default.into(),
            },
        });
        self
    }

    /// Add an enum dropdown parameter.
    #[must_use]
    pub fn with_enum(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        options: Vec<String>,
        default: impl Into<String>,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::Dropdown {
                options,
                default: default.into(),
            },
        });
        self
    }

    /// Add a boolean toggle parameter.
    #[must_use]
    pub fn with_bool(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        default: bool,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::Toggle { default },
        });
        self
    }

    /// Add a 2D point parameter.
    #[must_use]
    pub fn with_point(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        default: PointValue,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::Point2D { default },
        });
        self
    }

    /// Add an asset reference parameter.
    #[must_use]
    pub fn with_asset_ref(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        media_type: impl Into<String>,
        default_asset: Option<String>,
    ) -> Self {
        self.params.push(InspectorParam {
            id: id.into(),
            label: label.into(),
            keyframeable: true,
            widget: ControlWidget::AssetPicker {
                media_type: media_type.into(),
                default_asset,
            },
        });
        self
    }
}
