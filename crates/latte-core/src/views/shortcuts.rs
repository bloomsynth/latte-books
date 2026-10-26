//! The keyboard shortcuts the Shortcuts window lists: `src/components/ShortcutsHelper.vue`
//! and `getShortcutKeyMap` of `src/utils/ui.ts` @ v0.36.0.

/// `ShortcutKey`: a key named the same on every platform, shown as the platform names it.
pub const PMOD: &str = "pmod";
pub const CTRL: &str = "ctrl";
pub const SHIFT: &str = "shift";
pub const ALT: &str = "alt";
pub const DELETE: &str = "delete";
pub const ESC: &str = "esc";
pub const ENTER: &str = "enter";

/// One shortcut: its keys and what it does.
pub struct Shortcut {
    pub keys: &'static [&'static str],
    pub description: &'static str,
}

/// A group of shortcuts: where they apply.
pub struct ShortcutGroup {
    pub label: &'static str,
    pub description: &'static str,
    pub shortcuts: &'static [Shortcut],
}

const fn s(keys: &'static [&'static str], description: &'static str) -> Shortcut {
    Shortcut { keys, description }
}

/// The groups, as Books lists them. The descriptions of Entry's Save and Delete are
/// Books' sentences joined with a space.
pub const GROUPS: [ShortcutGroup; 6] = [
    ShortcutGroup {
        label: "Global",
        description: "Applicable anywhere in Latte Books",
        shortcuts: &[
            s(&[PMOD, "K"], "Open Quick Search"),
            s(&[SHIFT, DELETE], "Go back to the previous page"),
            s(&[SHIFT, "H"], "Toggle sidebar"),
            s(&["F1"], "Open Documentation"),
        ],
    },
    ShortcutGroup {
        label: "Entry",
        description: "Applicable when a entry is open in the Form view or Quick Edit view",
        shortcuts: &[
            s(
                &[PMOD, "S"],
                "Save or Submit an entry. An entry is submitted only if it is submittable and \
                 is in the saved state.",
            ),
            s(
                &[PMOD, DELETE],
                "Cancel or Delete an entry. An entry is cancelled only if it is in the \
                 submitted state. A submittable entry is deleted only if it is in the \
                 cancelled state.",
            ),
            s(&[PMOD, "P"], "Open Print View if Print is available."),
            s(
                &[PMOD, "L"],
                "Toggle Linked Entries widget, not available in Quick Edit view.",
            ),
        ],
    },
    ShortcutGroup {
        label: "List View",
        description: "Applicable when the List View of an entry type is open",
        shortcuts: &[
            s(
                &[PMOD, "N"],
                "Create a new entry of the same type as the List View",
            ),
            s(&[PMOD, "E"], "Open the Export Wizard modal"),
        ],
    },
    ShortcutGroup {
        label: "Quick Search",
        description: "Applicable when Quick Search is open",
        shortcuts: &[
            s(&[ESC], "Close Quick Search"),
            s(&[PMOD, "1"], "Toggle the Docs filter"),
            s(&[PMOD, "2"], "Toggle the List filter"),
            s(&[PMOD, "3"], "Toggle the Create filter"),
            s(&[PMOD, "4"], "Toggle the Report filter"),
            s(&[PMOD, "5"], "Toggle the Page filter"),
        ],
    },
    ShortcutGroup {
        label: "Template Builder",
        description: "Applicable when Template Builder is open",
        shortcuts: &[
            s(
                &[CTRL, ENTER],
                "Apply and view changes made to the print template",
            ),
            s(&[CTRL, "E"], "Toggle Edit Mode"),
            s(&[CTRL, "H"], "Toggle Key Hints"),
            s(&[CTRL, "+"], "Increase print template display scale"),
            s(&[CTRL, "-"], "Decrease print template display scale"),
        ],
    },
    ShortcutGroup {
        label: "Point of Sale",
        description: "Applicable when POS is open",
        shortcuts: &[
            s(&[SHIFT, "V"], "Toggle between Grid and List view"),
            s(&[SHIFT, "S"], "Open Sales Invoice List"),
            s(&[SHIFT, "L"], "Set Loyalty Program"),
            s(&[SHIFT, "C"], "Set Coupon Code"),
            s(&[SHIFT, "P"], "Set Price List"),
            s(
                &["Q", "0-9"],
                "Hold Q and type digits to set selected item quantity",
            ),
            s(&[PMOD, SHIFT, "H"], "Open Saved or Submitted Invoice List."),
            s(
                &[PMOD, SHIFT, "S"],
                "If any entry form is open, save the details. Otherwise, save the invoice.",
            ),
            s(&[PMOD, SHIFT, "P"], "Create Payment."),
            s(
                &[PMOD, SHIFT, DELETE],
                "If any entry form is open, your entry will be canceled. Otherwise the \
                 selected items will be removed.",
            ),
        ],
    },
];

/// `getShortcutKeyMap`: how a platform names a key; other keys are shown as they are.
pub fn key_label(key: &'static str, mac: bool) -> &'static str {
    match (key, mac) {
        (ALT, true) => "⌥",
        (CTRL, true) => "⌃",
        (PMOD, true) => "⌘",
        (SHIFT, true) => "shift",
        (DELETE, true) => "delete",
        (ESC, true) => "esc",
        (ENTER, true) => "return",
        (ALT, false) => "Alt",
        (CTRL | PMOD, false) => "Ctrl",
        (SHIFT, false) => "⇧",
        (DELETE, false) => "Backspace",
        (ESC, false) => "Esc",
        (ENTER, false) => "Enter",
        (key, _) => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_for_the_platform() {
        assert_eq!(key_label(PMOD, false), "Ctrl");
        assert_eq!(key_label(PMOD, true), "⌘");
        assert_eq!(key_label(SHIFT, false), "⇧");
        assert_eq!(key_label(DELETE, true), "delete");
        assert_eq!(key_label("K", true), "K");
    }

    #[test]
    fn the_groups_are_books() {
        let labels: Vec<&str> = GROUPS.iter().map(|g| g.label).collect();
        assert_eq!(
            labels,
            [
                "Global",
                "Entry",
                "List View",
                "Quick Search",
                "Template Builder",
                "Point of Sale"
            ]
        );
        assert_eq!(GROUPS[3].shortcuts.len(), 6);
        assert_eq!(GROUPS[5].shortcuts.len(), 10);
    }
}
