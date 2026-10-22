//! Cursor movement, deletion and input-method composition over Indic and CJK text, with real
//! key events in a headless window. Shaping and wrapping are checked from screenshots instead
//! (see RESULTS.md): the test platform does not shape text.

use gpui_kit::test::{TestInput, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, ElementInputHandler, Entity, Point, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};

use crate::{LANGUAGES, TextSpike, load_translations};

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<TextSpike>) {
    cx.update(gpui_kit::init);
    let opened = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1200.), px(1250.)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| TextSpike::new(window, cx))
        })
        .expect("open test window")
    });
    cx.update_window(opened.0, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    opened
}

/// Clicks the language's input, replaces its text, presses `keys`, and returns the byte offset
/// of the cursor after each key, plus the final text.
fn edit(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    spike: &Entity<TextSpike>,
    code: &'static str,
    text: &str,
    keys: &[&str],
) -> (Vec<usize>, String) {
    let state = cx.read(|cx| spike.read(cx).input(code));
    cx.update_window(handle, |_, window, cx| {
        state.update(cx, |input, cx| input.set_value(text.to_string(), window, cx));
        window.click(code, cx);
        window.press("home", cx);
        let offsets = keys
            .iter()
            .map(|key| {
                window.press(key, cx);
                state.read(cx).cursor()
            })
            .collect();
        (offsets, state.read(cx).value().to_string())
    })
    .unwrap()
}

// Known issue: gpui-kit's input moves the cursor by code point, so it can stop inside a conjunct.
// These tests describe the correct behaviour; run them with `--ignored` to check a new gpui-kit.
// gpui-base-grapheme-cursor.patch fixes it.

#[gpui_kit::test]
#[ignore = "known issue: the cursor moves by code point, not by cluster"]
fn right_arrow_steps_over_whole_conjuncts(cx: &mut TestAppContext) {
    let (handle, spike) = open(cx);
    // क्ष | त्रि | य: each Devanagari or Gujarati code point is 3 bytes.
    for (code, word) in [("hi", "क्षत्रिय"), ("gu", "ક્ષત્રિય")] {
        let (offsets, _) = edit(cx, handle, &spike, code, word, &["right"; 3]);
        assert_eq!(offsets, [9, 21, 24], "{code}: cursor stops in {word}");
    }
    // A Hangul syllable and a Han character are one code point (3 bytes) each.
    for (code, word) in [("ko", "똠방각"), ("zh-CN", "增值税")] {
        let (offsets, _) = edit(cx, handle, &spike, code, word, &["right"; 3]);
        assert_eq!(offsets, [3, 6, 9], "{code}");
    }
}

#[gpui_kit::test]
#[ignore = "known issue: the cursor moves by code point, not by cluster"]
fn left_arrow_respects_clusters(cx: &mut TestAppContext) {
    let (handle, spike) = open(cx);
    let (offsets, _) = edit(
        cx,
        handle,
        &spike,
        "hi",
        "श्रीमती",
        &["end", "left", "left", "left"],
    );
    // श्री | म | ती
    assert_eq!(offsets, [21, 15, 12, 0]);
}

#[gpui_kit::test]
fn backspace_removes_one_code_point(cx: &mut TestAppContext) {
    let (handle, spike) = open(cx);
    // Backspace at the end of "कि" (consonant + vowel sign) removes only the vowel sign, so it
    // can be fixed without retyping its consonant, as in Books (Chromium).
    let (_, text) = edit(cx, handle, &spike, "hi", "कि", &["end", "backspace"]);
    assert_eq!(text, "क");
    let (_, text) = edit(cx, handle, &spike, "ko", "한국", &["end", "backspace"]);
    assert_eq!(text, "한");
}

#[gpui_kit::test]
#[ignore = "known issue: Delete removes one code point and leaves a dangling virama"]
fn delete_removes_a_whole_conjunct(cx: &mut TestAppContext) {
    let (handle, spike) = open(cx);
    // Stock gpui-kit leaves "्षय".
    let (_, text) = edit(cx, handle, &spike, "hi", "क्षय", &["delete"]);
    assert_eq!(text, "य");
}

#[gpui_kit::test]
fn typing_devanagari_and_composing_cjk(cx: &mut TestAppContext) {
    let (handle, spike) = open(cx);
    // Typed text, as from the Windows Hindi (INSCRIPT) keyboard: one code point per key.
    let state = cx.read(|cx| spike.read(cx).input("hi"));
    cx.update_window(handle, |_, window, cx| {
        state.update(cx, |input, cx| input.set_value("", window, cx));
        window.click("hi", cx);
        for ch in ["न", "म", "स", "्", "त", "े"] {
            window.input(ch, cx);
        }
        assert_eq!(state.read(cx).value(), "नमस्ते");
    })
    .unwrap();

    // An input method composes text before committing it: pinyin for Chinese, jamo for Korean.
    for (code, steps, committed) in [
        ("zh-CN", &["z", "zh", "zhong"][..], "中"),
        ("ko", &["ㅎ", "하", "한"][..], "한"),
    ] {
        let state = cx.read(|cx| spike.read(cx).input(code));
        cx.update_window(handle, |_, window, cx| {
            state.update(cx, |input, cx| input.set_value("", window, cx));
            window.click(code, cx);
            let bounds = window.find(code).bounds();
            let mut ime = TestInput::new(ElementInputHandler::new(bounds, state.clone()), window);
            for step in steps {
                ime.compose_text(None, step, None, cx);
                assert!(ime.marked_text_range(cx).is_some(), "{code}: {step} is marked");
            }
            ime.commit_text(committed, cx);
            assert_eq!(ime.marked_text_range(cx), None);
            drop(ime);
            assert_eq!(state.read(cx).value(), committed, "{code}");
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn translations_load_from_books_files(cx: &mut TestAppContext) {
    let _ = cx;
    for language in &LANGUAGES {
        let translations = load_translations(language.csv);
        assert!(translations.len() > 500, "{}", language.code);
        assert!(translations.contains_key("Sales Invoice"), "{}", language.code);
    }
}
