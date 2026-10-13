#[test]
fn official_tokyo_https_json_renders_all_supported_tokyo_areas() {
    let bytes = include_bytes!("../assets/jma-official-130000-20261012T1700.json");
    let choices: Vec<_> = picomado_core::weather::LOCATIONS
        .iter()
        .filter(|choice| choice.office_code == "130000")
        .collect();
    assert_eq!(choices.len(), 4);
    for choice in choices {
        let set = picomado_core::weather::ForecastSet::for_location(
            bytes,
            "official-live",
            "2026-10-12",
            choice,
        )
        .unwrap();
        assert_eq!(set.days.len(), 7);
        assert_eq!(set.days[0].date_jst, "2026-10-12");
        let image = picomado_core::weather::render_lcd(&set, &set.days[0]).unwrap();
        assert_eq!(image.len(), picomado_core::protocol::LCD_FRAME_BYTES);
        assert!(image.iter().any(|byte| *byte != image[0]));
    }
}

#[test]
fn official_forecasts_across_japan_render_each_mapped_area() {
    let samples: [(&str, &[u8]); 3] = [
        (
            "011000",
            include_bytes!("../assets/jma-official-011000-20261012.json"),
        ),
        (
            "100000",
            include_bytes!("../assets/jma-official-100000-20261012.json"),
        ),
        (
            "400000",
            include_bytes!("../assets/jma-official-400000-20261012.json"),
        ),
    ];
    for (office, bytes) in samples {
        let choices: Vec<_> = picomado_core::weather::LOCATIONS
            .iter()
            .filter(|choice| choice.office_code == office)
            .collect();
        assert!(!choices.is_empty());
        for choice in choices {
            let set = picomado_core::weather::ForecastSet::for_location(
                bytes,
                "official-sample",
                "2026-10-12",
                choice,
            )
            .unwrap();
            assert_eq!(set.days.len(), 7);
            assert_eq!(
                picomado_core::weather::render_lcd(&set, &set.days[0])
                    .unwrap()
                    .len(),
                picomado_core::protocol::LCD_FRAME_BYTES
            );
        }
    }
}

#[test]
fn catalogue_has_unique_region_and_temperature_mappings() {
    let choices = &*picomado_core::weather::LOCATIONS;
    assert_eq!(choices.len(), 139);
    assert_eq!(
        choices
            .iter()
            .map(|choice| choice.office_code.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        56
    );
    assert_eq!(choices[0].id, "tokyo-station");
    let mut ids = std::collections::BTreeSet::new();
    for choice in choices {
        assert!(ids.insert(&choice.id));
        assert_eq!(choice.office_code.len(), 6);
        assert_eq!(choice.area_code.len(), 6);
        assert_eq!(choice.weekly_area_code.len(), 6);
        assert_eq!(choice.point_code.len(), 5);
        assert!(!choice.area_label.is_empty() && !choice.point_label.is_empty());
    }
}

#[test]
fn official_national_snapshot_renders_every_selectable_location() {
    let all: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../assets/jma-official-national-20261012.json"
    ))
    .unwrap();
    for choice in picomado_core::weather::LOCATIONS.iter() {
        let office = &all[&choice.office_code];
        assert!(!office.is_null(), "missing {}", choice.office_code);
        let bytes = serde_json::to_vec(office).unwrap();
        let set = picomado_core::weather::ForecastSet::for_location(
            &bytes,
            "national-snapshot",
            "2026-10-12",
            choice,
        )
        .unwrap_or_else(|e| panic!("{} {}: {e:?}", choice.id, choice.office_code));
        assert_eq!(set.days.len(), 7, "{}", choice.id);
        for day in &set.days {
            assert_eq!(
                picomado_core::weather::render_lcd(&set, day).unwrap().len(),
                picomado_core::protocol::LCD_FRAME_BYTES,
                "{} {}",
                choice.id,
                day.date_jst
            );
        }
    }
}
