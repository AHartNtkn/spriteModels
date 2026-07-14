use std::{error::Error, fmt};

use editor_core::{ActiveLayer, DepthValue, EditorDocument, ReliefValue, Tool};
use eframe::egui;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolEntry {
    pub tool: Tool,
    pub label: &'static str,
}

const TOOLS: [ToolEntry; 4] = [
    ToolEntry {
        tool: Tool::Pencil,
        label: "Pencil",
    },
    ToolEntry {
        tool: Tool::Eraser,
        label: "Eraser",
    },
    ToolEntry {
        tool: Tool::Fill,
        label: "Fill",
    },
    ToolEntry {
        tool: Tool::Eyedropper,
        label: "Eyedropper",
    },
];

pub fn tool_entries() -> impl ExactSizeIterator<Item = ToolEntry> {
    TOOLS.into_iter()
}

pub fn select_tool(document: &mut EditorDocument, tool: Tool) {
    document.set_tool(tool);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbChannelError(pub usize);

impl fmt::Display for RgbChannelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "RGB channel {} is outside 0..3", self.0)
    }
}

impl Error for RgbChannelError {}

pub fn set_rgb_channel(
    document: &mut EditorDocument,
    channel: usize,
    value: u8,
) -> Result<(), RgbChannelError> {
    let mut rgb = document.current_rgb();
    let destination = rgb.get_mut(channel).ok_or(RgbChannelError(channel))?;
    *destination = value;
    document.set_current_rgb(rgb);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HexColorError {
    Length,
    Digit,
}

impl fmt::Display for HexColorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length => {
                formatter.write_str("color must contain exactly six hexadecimal digits")
            }
            Self::Digit => formatter.write_str("color contains a non-hexadecimal digit"),
        }
    }
}

impl Error for HexColorError {}

pub fn parse_rgb_hex(input: &str) -> Result<[u8; 3], HexColorError> {
    if input.len() != 6 {
        return Err(HexColorError::Length);
    }
    let mut rgb = [0; 3];
    for (channel, pair) in input.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).map_err(|_| HexColorError::Digit)?;
        rgb[channel] = u8::from_str_radix(pair, 16).map_err(|_| HexColorError::Digit)?;
    }
    Ok(rgb)
}

pub fn format_rgb_hex(rgb: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub fn apply_hex_rgb(document: &mut EditorDocument, input: &str) -> Result<(), HexColorError> {
    document.set_current_rgb(parse_rgb_hex(input)?);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReliefLabels {
    pub units: String,
    pub model_pixels: String,
}

pub fn relief_labels(depth: DepthValue) -> ReliefLabels {
    match depth {
        DepthValue::Empty => ReliefLabels {
            units: "Empty".to_owned(),
            model_pixels: "No model surface".to_owned(),
        },
        DepthValue::Relief(relief) => {
            let units = relief.get();
            let pixels = f32::from(units) / 8.0;
            ReliefLabels {
                units: format!("{units} eighth-pixel units"),
                model_pixels: format!("{pixels} model pixels"),
            }
        }
    }
}

#[derive(Debug)]
pub struct PaletteState {
    hex_input: String,
    synced_rgb: [u8; 3],
    hex_error: bool,
}

impl PaletteState {
    pub fn new(document: &EditorDocument) -> Self {
        let synced_rgb = document.current_rgb();
        Self {
            hex_input: format_rgb_hex(synced_rgb),
            synced_rgb,
            hex_error: false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, document: &mut EditorDocument) {
        ui.vertical(|ui| {
            for entry in tool_entries() {
                if ui
                    .selectable_label(document.tool() == entry.tool, entry.label)
                    .clicked()
                {
                    select_tool(document, entry.tool);
                }
            }

            ui.separator();
            ui.label("Color");
            let mut rgb = document.current_rgb();
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                document.set_current_rgb(rgb);
            }
            for (channel, label) in ["R", "G", "B"].into_iter().enumerate() {
                let mut value = rgb[channel];
                ui.horizontal(|ui| {
                    ui.label(label);
                    if ui
                        .add(egui::DragValue::new(&mut value).range(0..=255))
                        .changed()
                    {
                        set_rgb_channel(document, channel, value)
                            .expect("palette RGB controls enumerate exactly three channels");
                    }
                });
            }

            if document.current_rgb() != self.synced_rgb
                && !ui.memory(|memory| memory.focused().is_some())
            {
                self.sync_hex(document.current_rgb());
            }
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.hex_input)
                    .char_limit(6)
                    .hint_text("RRGGBB"),
            );
            let submit = response.lost_focus()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            if submit {
                match apply_hex_rgb(document, &self.hex_input) {
                    Ok(()) => {
                        self.sync_hex(document.current_rgb());
                        self.hex_error = false;
                    }
                    Err(_) => self.hex_error = true,
                }
            }
            if self.hex_error {
                ui.colored_label(egui::Color32::LIGHT_RED, "Use six hex digits");
            }

            ui.separator();
            ui.label("Layer");
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(document.active_layer() == ActiveLayer::Color, "Color")
                    .clicked()
                {
                    document.set_active_layer(ActiveLayer::Color);
                }
                if ui
                    .selectable_label(document.active_layer() == ActiveLayer::Depth, "Depth")
                    .clicked()
                {
                    document.set_active_layer(ActiveLayer::Depth);
                }
            });

            let mut relief = match document.current_depth() {
                DepthValue::Empty => 0,
                DepthValue::Relief(value) => value.get(),
            };
            if ui
                .add(egui::Slider::new(&mut relief, 0..=254).text("relief"))
                .changed()
            {
                document.set_current_depth(DepthValue::Relief(
                    ReliefValue::new(relief).expect("slider enforces valid relief"),
                ));
            }
            let labels = relief_labels(document.current_depth());
            ui.small(labels.units);
            ui.small(labels.model_pixels);
        });
    }

    fn sync_hex(&mut self, rgb: [u8; 3]) {
        self.hex_input = format_rgb_hex(rgb);
        self.synced_rgb = rgb;
    }
}
