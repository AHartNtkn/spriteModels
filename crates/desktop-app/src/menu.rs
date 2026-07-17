use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    New,
    Open,
    ImportModel,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    ResetView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingDestructiveAction {
    New,
    Open(PathBuf),
    Import(relief_core::AuthoredModel),
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuGroup {
    File,
    Edit,
    View,
}

impl MenuGroup {
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::View => "View",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MenuItem {
    pub label: &'static str,
    pub action: MenuAction,
}

const FILE_ITEMS: [MenuItem; 6] = [
    MenuItem {
        label: "New",
        action: MenuAction::New,
    },
    MenuItem {
        label: "Open",
        action: MenuAction::Open,
    },
    MenuItem {
        label: "Import 3D Model…",
        action: MenuAction::ImportModel,
    },
    MenuItem {
        label: "Save",
        action: MenuAction::Save,
    },
    MenuItem {
        label: "Save As",
        action: MenuAction::SaveAs,
    },
    MenuItem {
        label: "Quit",
        action: MenuAction::Quit,
    },
];

const EDIT_ITEMS: [MenuItem; 2] = [
    MenuItem {
        label: "Undo",
        action: MenuAction::Undo,
    },
    MenuItem {
        label: "Redo",
        action: MenuAction::Redo,
    },
];

const VIEW_ITEMS: [MenuItem; 1] = [MenuItem {
    label: "Reset Model View",
    action: MenuAction::ResetView,
}];

pub const fn menu_items(group: MenuGroup) -> &'static [MenuItem] {
    match group {
        MenuGroup::File => &FILE_ITEMS,
        MenuGroup::Edit => &EDIT_ITEMS,
        MenuGroup::View => &VIEW_ITEMS,
    }
}

pub(crate) struct MenuBarOutput {
    pub action: Option<MenuAction>,
    #[cfg(test)]
    pub observation: MenuObservation,
}

#[cfg(test)]
pub(crate) struct MenuObservation {
    pub rect: eframe::egui::Rect,
}

pub(crate) fn show_menu_bar(ui: &mut eframe::egui::Ui) -> MenuBarOutput {
    let mut selected = None;
    let menu = eframe::egui::MenuBar::new().ui(ui, |ui| {
        for group in [MenuGroup::File, MenuGroup::Edit, MenuGroup::View] {
            ui.menu_button(group.label(), |ui| {
                for item in menu_items(group) {
                    if ui.button(item.label).clicked() {
                        selected = Some(item.action);
                        ui.close();
                    }
                }
            });
        }
    });
    #[cfg(not(test))]
    let _ = &menu;
    MenuBarOutput {
        action: selected,
        #[cfg(test)]
        observation: MenuObservation {
            rect: menu.response.rect,
        },
    }
}
