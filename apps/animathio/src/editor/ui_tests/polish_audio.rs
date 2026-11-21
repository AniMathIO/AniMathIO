//! Audio UI polish (follow-up to #318): Auto duck music defaults.

use gpui::{Modifiers, TestAppContext};

use super::Ui;
use super::fixtures::*;

/// Toggle the voice clip twice through the real "Use the selected clip as the voice" button:
/// once to clear it, once to set it again (which refreshes the music defaults).
fn clear_and_reset_voice(ui: &mut Ui) {
    for _ in 0..2 {
        let button = ui.cx.debug_bounds("duck-voice-clip").expect("voice button");
        ui.click(button.center(), Modifiers::default());
        ui.settle();
    }
}

#[gpui::test]
fn switching_the_voice_clip_re_suggests_music_only_without_user_picks(cx: &mut TestAppContext) {
    let (project, assets) = voice_and_music();
    let mut ui = Ui::open(cx, project, assets);
    ui.update(|e, _, cx| {
        e.select_only(VOICE_CLIP, cx);
        e.open_duck(cx);
    });
    ui.settle();
    ui.read(|e| {
        let params = &e.duck_review.as_ref().unwrap().params;
        assert_eq!(params.voice_clips, vec![VOICE_CLIP]);
        assert_eq!(params.music_tracks, vec![MUSIC_TRACK]);
    });

    // No music picked at all: choosing the voice again suggests the other audio tracks again.
    ui.update(|e, _, _| {
        let params = &mut e.duck_review.as_mut().unwrap().params;
        params.music_tracks.clear();
        params.music_clips.clear();
    });
    clear_and_reset_voice(&mut ui);
    ui.read(|e| {
        let params = &e.duck_review.as_ref().unwrap().params;
        assert_eq!(params.voice_clips, vec![VOICE_CLIP]);
        assert_eq!(params.music_tracks, vec![MUSIC_TRACK]);
        assert_eq!(params.music_clips, []);
    });

    // The user picked a music clip (and no track): that choice is kept, nothing is suggested.
    ui.update(|e, _, _| {
        let params = &mut e.duck_review.as_mut().unwrap().params;
        params.music_tracks.clear();
        params.music_clips = vec![MUSIC_CLIP];
    });
    clear_and_reset_voice(&mut ui);
    ui.read(|e| {
        let params = &e.duck_review.as_ref().unwrap().params;
        assert_eq!(params.music_tracks, []);
        assert_eq!(params.music_clips, vec![MUSIC_CLIP]);
    });

    // A music pick that becomes the voice is dropped.
    ui.update(|e, _, _| {
        let params = &mut e.duck_review.as_mut().unwrap().params;
        params.music_clips = vec![VOICE_CLIP];
    });
    clear_and_reset_voice(&mut ui);
    ui.read(|e| {
        let params = &e.duck_review.as_ref().unwrap().params;
        assert_eq!(params.music_clips, []);
    });
}
