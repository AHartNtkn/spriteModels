use relief_render::{DirectionCount, SheetError, SheetRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DirectionChoice {
    Eight,
    Sixteen,
}

impl DirectionChoice {
    pub(crate) const ALL: [Self; 2] = [Self::Eight, Self::Sixteen];

    fn render_count(self) -> DirectionCount {
        match self {
            Self::Eight => DirectionCount::Eight,
            Self::Sixteen => DirectionCount::Sixteen,
        }
    }
}

pub(crate) struct ExportOptions {
    direction: DirectionChoice,
    integer_scale: u32,
    padding: u32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            direction: DirectionChoice::Eight,
            integer_scale: 1,
            padding: 0,
        }
    }
}

impl ExportOptions {
    pub(crate) fn direction(&self) -> DirectionChoice {
        self.direction
    }

    pub(crate) fn set_direction(&mut self, direction: DirectionChoice) {
        self.direction = direction;
    }

    pub(crate) fn integer_scale(&self) -> u32 {
        self.integer_scale
    }

    pub(crate) fn set_integer_scale(&mut self, scale: u32) -> Result<(), SheetError> {
        SheetRequest::new(self.direction.render_count(), scale, self.padding, 1)?;
        self.integer_scale = scale;
        Ok(())
    }

    pub(crate) fn padding(&self) -> u32 {
        self.padding
    }

    pub(crate) fn set_padding(&mut self, padding: u32) {
        self.padding = padding;
    }

    pub(crate) fn elevation_index(&self) -> u8 {
        1
    }

    pub(crate) fn request(&self) -> Result<SheetRequest, SheetError> {
        SheetRequest::new(
            self.direction.render_count(),
            self.integer_scale,
            self.padding,
            self.elevation_index(),
        )
    }
}

pub(crate) fn show(ui: &mut eframe::egui::Ui, options: &mut ExportOptions) -> bool {
    ui.heading("Export sheet");
    let mut direction = options.direction();
    eframe::egui::ComboBox::from_label("Directions")
        .selected_text(match direction {
            DirectionChoice::Eight => "8",
            DirectionChoice::Sixteen => "16",
        })
        .show_ui(ui, |ui| {
            for choice in DirectionChoice::ALL {
                let label = match choice {
                    DirectionChoice::Eight => "8",
                    DirectionChoice::Sixteen => "16",
                };
                ui.selectable_value(&mut direction, choice, label);
            }
        });
    options.set_direction(direction);

    let mut integer_scale = options.integer_scale();
    if ui
        .add(
            eframe::egui::DragValue::new(&mut integer_scale)
                .range(1..=8)
                .speed(1.0)
                .prefix("Integer scale: "),
        )
        .changed()
    {
        options
            .set_integer_scale(integer_scale)
            .expect("the UI constrains scale to positive integers");
    }
    let mut padding = options.padding();
    if ui
        .add(
            eframe::egui::DragValue::new(&mut padding)
                .range(0..=128)
                .speed(1.0)
                .prefix("Padding: "),
        )
        .changed()
    {
        options.set_padding(padding);
    }
    ui.label("Elevation: v1 fixed");
    ui.button("Export PNG…").clicked()
}

#[cfg(test)]
mod tests {
    use relief_render::DirectionCount;

    use super::{DirectionChoice, ExportOptions};

    #[test]
    fn export_options_expose_only_eight_or_sixteen_and_v1_elevation() {
        let mut options = ExportOptions::default();
        assert_eq!(options.direction(), DirectionChoice::Eight);
        assert_eq!(options.elevation_index(), 1);
        assert_eq!(
            options.request().unwrap().direction_count(),
            DirectionCount::Eight
        );

        options.set_direction(DirectionChoice::Sixteen);
        assert_eq!(
            options.request().unwrap().direction_count(),
            DirectionCount::Sixteen
        );
        assert_eq!(DirectionChoice::ALL.len(), 2);
    }

    #[test]
    fn export_scale_is_positive_integer_and_padding_is_integer() {
        let mut options = ExportOptions::default();
        options.set_integer_scale(3).unwrap();
        options.set_padding(7);
        let request = options.request().unwrap();
        assert_eq!(request.integer_scale(), 3);
        assert_eq!(request.padding(), 7);

        assert!(options.set_integer_scale(0).is_err());
        assert_eq!(options.integer_scale(), 3);
    }
}
