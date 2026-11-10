//! Runs the Node checks for `ui/js/player-colors.js`, and pins the storage key
//! / Config wiring so Your color stays backward compatible.

#[test]
fn player_color_palette_assignment_via_node() {
    let status = std::process::Command::new("node")
        .arg("tests/player_colors_node.js")
        .status()
        .expect("node must be available to test player-colors.js");
    assert!(status.success(), "player_colors_node.js failed");
}

#[test]
fn your_color_lives_in_config_with_the_history_storage_key() {
    let colors = include_str!("../ui/js/player-colors.js");
    let meter_html = include_str!("../ui/index.html");
    let config_schema = include_str!("../ui/js/config-schema.js");
    let config_js = include_str!("../ui/js/config-panel.js");
    let history_html = include_str!("../ui/history.html");
    let history_js = include_str!("../ui/js/history.js");
    let overlay_js = include_str!("../ui/js/overlay.js");
    let overlay_html = include_str!("../ui/overlay.html");

    assert!(colors.contains("aion2dps.history.youColor"));
    assert!(colors.contains("function colorsFor("));
    assert!(colors.contains("function readYouColor("));
    assert!(colors.contains("function writeYouColor("));

    assert!(config_schema.contains("id: 'you_color'"));
    assert!(config_schema.contains("label: 'Your color'"));
    assert!(config_js.contains("pcApi.writeYouColor"));
    assert!(meter_html.contains("player-colors.js"));

    assert!(
        !history_html.contains("id=\"you-color\""),
        "Your color control moved out of session history"
    );
    assert!(history_js.contains("playerColors.readYouColor"));
    assert!(!history_js.contains("function setYouColor"));

    assert!(overlay_html.contains("player-colors.js"));
    assert!(overlay_js.contains("playerColors.colorsFor"));
    assert!(overlay_js.contains("--pc:${color}"));
}
