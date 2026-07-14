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

pub struct PaletteOutput {
    #[cfg(test)]
    pub(crate) observation: PaletteObservation,
}

#[cfg(test)]
pub(crate) struct PaletteObservation {
    pub rect: egui::Rect,
    pub controls: Vec<egui::Rect>,
    pub color_popover: Option<ColorPopoverObservation>,
}

#[cfg(test)]
pub(crate) struct ColorPopoverObservation {
    pub rect: egui::Rect,
    pub picker: egui::Rect,
    pub rgb_rows: [egui::Rect; 3],
    pub rgb_labels: [egui::Rect; 3],
    pub hex: egui::Rect,
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

    pub fn show(&mut self, ui: &mut egui::Ui, document: &mut EditorDocument) -> PaletteOutput {
        #[cfg(test)]
        let mut controls = Vec::new();
        #[cfg(test)]
        let mut color_popover = None;
        let palette = ui.vertical(|ui| {
            for entry in tool_entries() {
                let selected = document.tool() == entry.tool;
                let response = ui
                    .add_enabled_ui(entry.tool.is_available_on(document.active_layer()), |ui| {
                        ui.add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::selectable(selected, entry.label),
                        )
                    })
                    .inner
                    .on_hover_text(entry.label);
                #[cfg(test)]
                controls.push(response.rect);
                if response.clicked() {
                    select_tool(document, entry.tool);
                }
            }

            ui.separator();
            let color_response = ui.menu_button("Color", |ui| {
                ui.set_min_width(170.0);
                let rgb = document.current_rgb();
                let mut color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                let picker = ui.scope(|ui| {
                    egui::color_picker::color_picker_color32(
                        ui,
                        &mut color,
                        egui::color_picker::Alpha::Opaque,
                    )
                });
                if picker.inner {
                    let [red, green, blue, _] = color.to_array();
                    document.set_current_rgb([red, green, blue]);
                }

                let rgb = document.current_rgb();
                #[cfg(test)]
                let mut rgb_rows = [egui::Rect::NOTHING; 3];
                #[cfg(test)]
                let mut rgb_labels = [egui::Rect::NOTHING; 3];
                for (channel, label) in ["R", "G", "B"].into_iter().enumerate() {
                    let mut value = rgb[channel];
                    let _row = ui.horizontal(|ui| {
                        let _label_response = ui.label(label);
                        #[cfg(test)]
                        {
                            rgb_labels[channel] = _label_response.rect;
                        }
                        if ui
                            .add(egui::DragValue::new(&mut value).range(0..=255))
                            .changed()
                        {
                            set_rgb_channel(document, channel, value)
                                .expect("palette RGB controls enumerate exactly three channels");
                        }
                    });
                    #[cfg(test)]
                    {
                        rgb_rows[channel] = _row.response.rect;
                    }
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
                    || (response.has_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)));
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
                #[cfg(test)]
                {
                    color_popover = Some(ColorPopoverObservation {
                        rect: ui.min_rect(),
                        picker: picker.response.rect,
                        rgb_rows,
                        rgb_labels,
                        hex: response.rect,
                    });
                }
            });
            #[cfg(test)]
            controls.push(color_response.response.rect);
            #[cfg(not(test))]
            let _ = &color_response;

            ui.separator();
            let color_layer_response = ui
                .selectable_label(document.active_layer() == ActiveLayer::Color, "Color Layer")
                .on_hover_text("Edit color");
            #[cfg(test)]
            controls.push(color_layer_response.rect);
            if color_layer_response.clicked() {
                document.set_active_layer(ActiveLayer::Color);
            }
            let depth_layer_response = ui
                .selectable_label(document.active_layer() == ActiveLayer::Depth, "Depth Layer")
                .on_hover_text("Edit relief");
            #[cfg(test)]
            controls.push(depth_layer_response.rect);
            if depth_layer_response.clicked() {
                document.set_active_layer(ActiveLayer::Depth);
            }

            let depth_response = ui.menu_button("Relief", |ui| {
                ui.set_min_width(190.0);
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
                ui.label(labels.units);
                ui.label(labels.model_pixels);
            });
            #[cfg(test)]
            controls.push(depth_response.response.rect);
            #[cfg(not(test))]
            let _ = &depth_response;
        });
        #[cfg(not(test))]
        let _ = &palette;
        PaletteOutput {
            #[cfg(test)]
            observation: PaletteObservation {
                rect: palette.response.rect,
                controls,
                color_popover,
            },
        }
    }

    fn sync_hex(&mut self, rgb: [u8; 3]) {
        self.hex_input = format_rgb_hex(rgb);
        self.synced_rgb = rgb;
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, Rect, pos2};
    use relief_core::{Bounds, CanonicalView};

    use super::*;

    fn run_frame(
        context: &egui::Context,
        palette: &mut PaletteState,
        document: &mut EditorDocument,
        events: Vec<Event>,
    ) -> (egui::FullOutput, PaletteObservation) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_max(Pos2::ZERO, pos2(360.0, 500.0))),
            events,
            ..Default::default()
        };
        let mut observation = None;
        let output = context.run_ui(input, |ui| {
            ui.set_width(crate::layout::TOOL_COLUMN_WIDTH);
            observation = Some(palette.show(ui, document).observation);
        });
        (output, observation.unwrap())
    }

    fn pointer_button(position: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    fn painted_text_rect(output: &egui::FullOutput, text: &str) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some(shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("palette did not paint {text:?}"))
    }

    #[test]
    fn color_button_opens_embedded_picker_rgb_and_hex_controls_with_contained_labels() {
        let context = egui::Context::default();
        let mut document = EditorDocument::new(Bounds::new(1, 1, 1).unwrap(), CanonicalView::Front);
        let mut palette = PaletteState::new(&document);

        let (initial, initial_observation) =
            run_frame(&context, &mut palette, &mut document, Vec::new());
        let color_button = initial_observation.controls[4];
        assert!(
            color_button.contains_rect(painted_text_rect(&initial, "Color")),
            "the visible Color label remains inside its button"
        );

        let center = color_button.center();
        run_frame(
            &context,
            &mut palette,
            &mut document,
            vec![Event::PointerMoved(center), pointer_button(center, true)],
        );
        let (_opened, opened_observation) = run_frame(
            &context,
            &mut palette,
            &mut document,
            vec![pointer_button(center, false)],
        );
        let popover = opened_observation
            .color_popover
            .expect("clicking Color opens its real egui popover");

        assert!(popover.rect.is_positive());
        assert!(popover.picker.is_positive());
        assert!(popover.rect.contains_rect(popover.picker));
        assert!(popover.rect.contains_rect(popover.hex));
        assert_eq!(palette.hex_input, "000000");
        for (index, label) in ["R", "G", "B"].into_iter().enumerate() {
            assert!(popover.rgb_rows[index].is_positive());
            assert!(popover.rect.contains_rect(popover.rgb_rows[index]));
            assert!(popover.rgb_rows[index].contains_rect(popover.rgb_labels[index]));
            assert!(
                popover.rgb_labels[index].is_positive(),
                "the {label} label is rendered"
            );
        }
    }
}
