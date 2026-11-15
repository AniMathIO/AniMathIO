//! CLI access to the same mathematical-object definitions as the editor.
use super::{
    BTreeMap, PathBuf, Result, Subcommand, TICKS_PER_SECOND, Time, load, parse_time, plugin_host,
    save_amio,
};
use animathio_control::{MathObjectDraft, MathObjectKind, math_object_command};

#[derive(Subcommand)]
pub(super) enum MathAction {
    /// Print default definitions for every supported object kind.
    List,
    /// Add an object to a free layer and save a new archive.
    Add {
        project: PathBuf,
        /// latex, text, point, line, vector, polygon, plot, circle, ellipse, cartesian, polar.
        #[arg(long)]
        kind: String,
        /// Field override, e.g. --field radius=2 or --field 'vertices=-2,-1; 2,-1; 0,2'.
        #[arg(long = "field")]
        fields: Vec<String>,
        /// Hex colour.
        #[arg(long)]
        color: Option<String>,
        /// Timeline start in seconds or ticks.
        #[arg(long, default_value = "0", value_parser = parse_time)]
        start: Time,
        /// Duration in seconds or ticks.
        #[arg(long, default_value = "5", value_parser = parse_time)]
        duration: Time,
        /// Apply the animation plugin's Write preset to LaTeX.
        #[arg(long)]
        write_on: bool,
        #[arg(short, long)]
        output: PathBuf,
    },
}

pub(super) fn run(action: MathAction) -> Result<()> {
    match action {
        MathAction::List => println!(
            "{}",
            serde_json::to_string_pretty(&MathObjectKind::ALL.map(MathObjectDraft::new))?
        ),
        MathAction::Add {
            project,
            kind,
            fields,
            color,
            start,
            duration,
            write_on,
            output,
        } => {
            let kind: MathObjectKind = serde_json::from_value(serde_json::Value::String(kind))?;
            let mut draft = MathObjectDraft::new(kind);
            for field in fields {
                let (key, value) = field.split_once('=').ok_or("field must be name=value")?;
                if !draft.fields.contains_key(key) {
                    return Err(format!("unknown field {key}").into());
                }
                draft.fields.insert(key.into(), value.into());
            }
            if let Some(color) = color {
                draft.color = color;
            }
            draft.write_on = write_on;
            let mut bundle = load(&project)?;
            let range = animathio_core::TimeRange::new(
                start,
                Time(start.0.checked_add(duration.0).ok_or("time overflow")?),
            )?;
            let (command, added) = math_object_command(&bundle.project, &draft, range, None)?;
            let mut host = plugin_host()?;
            let (plugin, mut params) = draft.parameters((
                bundle.project.width().max(1),
                bundle.project.height().max(1),
            ))?;
            if matches!(kind, MathObjectKind::Latex | MathObjectKind::Plot) {
                params["strict_errors"] = serde_json::json!(true);
            }
            host.render_clip(
                &plugin,
                &BTreeMap::new(),
                &params,
                0,
                (
                    bundle.project.width().max(1),
                    bundle.project.height().max(1),
                ),
            )?;
            let mut candidate = bundle.project.clone();
            command.apply(&mut candidate)?;
            let mut commands = vec![command];
            if write_on {
                if kind != MathObjectKind::Latex {
                    return Err("--write-on applies to LaTeX".into());
                }
                let (animation, _) = animathio_control::animation_preset_command(
                    &candidate,
                    Some(&host),
                    &animathio_control::AnimationApplyParams {
                        clip: added.clip_id,
                        preset: "typewriter-in".into(),
                        plugin: None,
                        duration: Some(Time(TICKS_PER_SECOND)),
                        options: BTreeMap::new(),
                    },
                )?;
                commands.push(animation);
            }
            animathio_core::Command::Batch { commands }.apply(&mut bundle.project)?;
            save_amio(&output, &bundle.project, &bundle.assets)?;
            println!("{}", serde_json::to_string(&added)?);
        }
    }
    Ok(())
}
