# Spike 4: Hindi, Gujarati, Chinese and Korean text

**Question:** does GPUI (through gpui-kit) render and edit Hindi, Gujarati, Chinese and Korean
text correctly: shaping, line wrapping, cursor movement, deletion and input methods?
**Plan reference:** PLAN.md, Phase 0, spike 4.

## What was built

`src/main.rs` (gpui-kit `=0.8.0`, GPUI `gpui-pre 0.3.9`) shows one panel per language:

- Books v0.36.0 labels ("Sales Invoice", "Customer", "Grand Total", …) looked up in the
  vendored translation files `vendor/books/translations/{hi,gu,zh-CN,ko}.csv`. Missing
  translations fall back to English, as in Books.
- A customer name and an amount in the local currency.
- A line of hard shaping cases, in regular and bold: conjuncts (क्ष, ज्ञ, त्र), reph (अर्थ),
  pre-base vowel signs (कि), nukta (ड़), ॐ, full-width CJK punctuation, rare Hangul syllables.
- A long settings description wrapped in a 300 px box.
- A gpui-kit `Input` holding the hard line, to try the cursor and editing.

`reference.html` shows the same strings in Chromium, which is what Books renders with.

Books' translation coverage at v0.36.0: Hindi 100% (1,212 of 1,212), Simplified Chinese 94%,
Gujarati 62% (659 of 1,058), Korean 50%.

## Rendering

Screenshots at 150% scaling on Windows 11, in `screenshots/`:

| File | What it shows |
|---|---|
| `chromium-reference.png` | `reference.html` in headless Edge |
| `gpui-before-fix.png` | The spike on stock GPUI |
| `gpui-with-fix.png` | The spike with `gpui-line-wrap.patch` applied, for comparison |

- **Shaping passes.** GPUI uses DirectWrite, with system font fallback to Nirmala UI for
  Devanagari and Gujarati and to Microsoft YaHei and Malgun Gothic for CJK. Every hard case
  matches Chromium glyph for glyph, in regular and bold.
- **Known issue: Hindi and Gujarati wrap in the middle of words.** GPUI's
  `LineWrapper::is_word_char` (`gpui/src/text_system/line_wrapper.rs`) is a hardcoded list of
  scripts that form words. It includes Bengali but no other Indic script, so GPUI treats every
  Devanagari and Gujarati character as a break opportunity and splits words, and even
  conjuncts, at the edge of a box. Text that fits on one line, like labels and most field
  values, is unaffected. `gpui-line-wrap.patch` shows the fix: widen the Bengali range to all
  Brahmic scripts of India and Sri Lanka (U+0900–U+0DFF) and add ZWNJ and ZWJ. With it, Hindi
  and Gujarati wrap between words as in Chromium (`gpui-with-fix.png`). We don't carry the
  patch; it is to be reported to Zed.
- Chinese wraps between characters, and Korean breaks within words where Chromium does too.
  Both are correct on stock GPUI.

## Editing: UI tests

`src/tests.rs` has 6 tests built on gpui-kit's test harness. They send real key events and
input-method calls to a headless window. The test platform does not shape text, so they check
offsets and values, not pixels.

| Test | Covers | Stock gpui-kit |
|---|---|---|
| `typing_devanagari_and_composing_cjk` | Typing न म स ् त े gives नमस्ते; pinyin composition commits 中; Hangul composition commits 한, with the text marked while composing | Pass |
| `backspace_removes_one_code_point` | Backspace after कि leaves क; Backspace after 한국 leaves 한 | Pass |
| `translations_load_from_books_files` | Each CSV loads over 500 translations, including "Sales Invoice" | Pass |
| `right_arrow_steps_over_whole_conjuncts` | → in क्षत्रिय / ક્ષત્રિય stops after क्ष, त्रि, य; one step per Hangul syllable and Han character | Known issue, ignored |
| `left_arrow_respects_clusters` | ← in श्रीमती stops before ती, म, श्री | Known issue, ignored |
| `delete_removes_a_whole_conjunct` | Delete before क्षय leaves य | Known issue, ignored |

The ignored tests describe the correct behaviour. `cargo test -p indic-cjk-text -- --ignored`
runs them, to check whether a new gpui-kit release has fixed the issue.

**Known issue: the cursor can stop inside a conjunct.** gpui-base's
`InputState::next_boundary` and `previous_boundary` only snap to char boundaries, so the arrow
keys move by code point. → takes 8 presses to cross क्षत्रिय, and the cursor can sit between a
consonant and its virama, where nothing sensible can be typed. Delete before क्षय removes only
क and leaves a dangling virama (्षय). Chinese and Korean are unaffected: each character is one
code point. Typing, which is how most text gets entered, works correctly.

`gpui-base-grapheme-cursor.patch` shows the fix: the arrow keys and Delete step by extended
grapheme cluster (`unicode-segmentation`, which implements Unicode 16's rule for Indic
conjuncts, so क्ष is one cluster). With it, all 6 tests pass. We don't carry the patch; it is
to be reported to gpui-kit.

Backspace removing one code point is correct, not part of the issue. Chromium, and so Books,
does the same: Backspace after कि removes the vowel sign and leaves क, so a wrong matra can be
fixed without retyping its consonant.

## Verdict

**Pass, with two known issues for Hindi and Gujarati.** Shaping is correct, and input-method
composition, which Chinese and Korean depend on, works. We use stock GPUI and gpui-kit and
don't patch them. Until upstream fixes them, Latte Books lists these as known issues:

1. Hindi and Gujarati text that wraps onto several lines can break in the middle of a word.
2. In text fields, the arrow keys and Delete can stop inside a Hindi or Gujarati conjunct.

Each fix is a few lines, kept here as a `.patch` file to send upstream.

## Still open

- **Report both issues upstream:** the line wrapper to Zed (GPUI), the cursor to gpui-kit.
  Check them again at each gpui-kit upgrade.
- **Not tried by hand with a real input method.** The tests drive the input-method interface
  directly. The Windows Hindi, Gujarati, Microsoft Pinyin and Korean IMEs should be tried in
  the release build, including the candidate window position.
- **Backspace behaviour was checked against Chromium's documented behaviour,** not by typing in
  Books itself.
- **Width is approximate when wrapping.** GPUI sums per-character advance widths to decide
  where to wrap, which ignores the narrower shaped width of conjuncts. Lines may break a little
  early; no case in the spike broke visibly wrong.
- **Fonts on other systems.** Only Windows 11 was checked. Linux needs Noto Sans Devanagari,
  Gujarati and CJK installed, or bundled; macOS has its own system fonts.
- **Number and date formats** (Indian digit grouping, local digits) are a Books formatting
  question, not a GPUI one, and are left to Phase 1.
