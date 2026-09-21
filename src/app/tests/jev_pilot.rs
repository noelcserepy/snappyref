//! Bounded experiment, not a release gate. All inputs go through the full-app
//! inspect harness; the final oracle is deliberately separate from Jev's state.
use super::*;
use egui_kittest::kittest::NodeT;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const GOAL: &str = "Import one copy of each supplied sample image onto the empty board. Put both images in one group named Reference, arranged as a Column. Finish with no text field focused. Do not import extra copies. Choose done only when the whole goal is satisfied.";
const MAX_STEPS: usize = 24;
const CMD: egui::Modifiers = egui::Modifiers {
    command: true,
    ..egui::Modifiers::NONE
};

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Input {
    Drop { sample: usize, at: [f32; 2] },
    Click { at: [f32; 2], shift: bool },
    Key(String),
    Text(String),
    Fill(String),
    Wait,
    Done,
    Blocked,
}

#[derive(Clone)]
struct OptionInput {
    id: String,
    description: String,
    input: Input,
}

fn advance(h: &mut Harness<'_, SnappyRefApp>) {
    // A bounded frame window, not run-to-idle. Import remains asynchronous.
    for _ in 0..4 {
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn apply(h: &mut Harness<'_, SnappyRefApp>, input: &Input, samples: &[PathBuf]) {
    match input {
        Input::Drop { sample, at } => {
            h.hover_at(egui::pos2(at[0], at[1]));
            h.input_mut().dropped_files = vec![egui::DroppedFile {
                path: Some(samples[*sample].clone()),
                ..Default::default()
            }];
        }
        Input::Click { at, shift } => {
            let at = egui::pos2(at[0], at[1]);
            h.hover_at(at);
            inspect_click_at(
                h,
                at,
                if *shift {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                },
            );
        }
        Input::Text(text) => h.event(egui::Event::Text(text.clone())),
        Input::Fill(text) => {
            h.key_press_modifiers(CMD, egui::Key::A);
            h.step();
            h.event(egui::Event::Text(text.clone()));
        }
        Input::Key(name) => {
            let (modifiers, key) = match name.as_str() {
                "select_all" => (CMD, egui::Key::A),
                "group" => (CMD, egui::Key::G),
                "group_picker" => (egui::Modifiers::NONE, egui::Key::G),
                "escape" => (egui::Modifiers::NONE, egui::Key::Escape),
                "enter" => (egui::Modifiers::NONE, egui::Key::Enter),
                "tab" => (egui::Modifiers::NONE, egui::Key::Tab),
                "undo" => (CMD, egui::Key::Z),
                "redo" => (egui::Modifiers { shift: true, ..CMD }, egui::Key::Z),
                _ => panic!("unknown recorded key"),
            };
            h.key_press_modifiers(modifiers, key);
        }
        Input::Wait => std::thread::sleep(Duration::from_millis(150)),
        Input::Done | Input::Blocked => {}
    }
    advance(h);
}

fn rect_value(r: egui::Rect) -> Value {
    json!([r.min.x, r.min.y, r.max.x, r.max.y])
}

/// Pilot restriction: axis-aligned samples, a closed sidebar, no search/dialogs.
/// Geometry is supplemental document data, not a claim about correct pixels.
/// Accessibility values come from the last rendered frame, including drafts.
fn observe(h: &Harness<'_, SnappyRefApp>) -> (Value, Vec<OptionInput>) {
    let app = h.state();
    let canvas = &app.canvas;
    let viewport = canvas.viewport_state();
    let pan = egui::vec2(viewport.camera_pan_x, viewport.camera_pan_y);
    let zoom = viewport.camera_zoom;
    let screen = test_screen_rect();
    let to_screen = |p: egui::Pos2| screen.center() + pan + p.to_vec2() * zoom;
    let mut options = Vec::new();
    let mut controls = Vec::new();
    for node in h.root().children_recursive() {
        let ax = node.accesskit_node();
        let Some(bounds) = ax.bounding_box() else {
            continue;
        };
        let rect = egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        );
        if !screen.intersects(rect) {
            continue;
        }
        let role = ax.role();
        if !matches!(
            role,
            egui::accesskit::Role::Button
                | egui::accesskit::Role::TextInput
                | egui::accesskit::Role::Label
                | egui::accesskit::Role::RadioButton
                | egui::accesskit::Role::SpinButton
        ) {
            continue;
        }
        let id = format!("widget:{:?}", ax.id());
        let label = ax.label().unwrap_or_default();
        let value = ax.value();
        // egui paints selectable_value as a Button but omits its selected AX
        // property. Supplement only the three controls actually in this frame.
        let selected = ax.is_selected().or_else(|| {
            matches!(label.as_str(), "Row" | "Column" | "Wrap")
                .then(|| {
                    canvas
                        .groups()
                        .iter()
                        .find(|g| canvas.selected_group_ids() == [g.id])
                })
                .flatten()
                .map(|g| format!("{:?}", g.flow) == label)
        });
        controls.push(
            json!({"id": id, "role": format!("{role:?}"), "label": label,
            "value": value, "selected": selected, "toggled": ax.toggled().map(|v| format!("{v:?}")),
            "text_selection": ax.raw_text_selection().map(|v| format!("{v:?}")), "focused": ax.is_focused(),
            "enabled": !ax.is_disabled(), "screen_rect": rect_value(rect)}),
        );
        if role != egui::accesskit::Role::Label
            && !ax.is_disabled()
            && screen.contains(rect.center())
        {
            options.push(OptionInput {
                id,
                description: format!("Click {role:?} labelled {label:?}, current value {value:?}"),
                input: Input::Click {
                    at: [rect.center().x, rect.center().y],
                    shift: false,
                },
            });
        }
    }
    let placements: Vec<_> = canvas.document.placements.iter().filter_map(|p| {
        let geometry = p.state.geometry;
        let bounds = geometry.visual_bounds();
        let rect = egui::Rect::from_min_max(to_screen(bounds.min), to_screen(bounds.max));
        if !screen.intersects(rect) { return None; }
        let center = to_screen(geometry.center);
        let reachable = app.canvas_input_policy(&h.ctx) == crate::canvas::CanvasInputPolicy::Full
            && screen.contains(center)
            && matches!(crate::canvas::inspect_hit_test_canvas_item(canvas, screen, center), Some(crate::canvas::CanvasHit::Placement(id)) if id == p.id())
            && h.ctx.layer_id_at(center).is_none_or(|layer| layer.order == egui::Order::Background);
        for shift in [false, true].into_iter().filter(|_| reachable) {
            options.push(OptionInput { id: format!("placement:{}:{}", p.id(), if shift { "shift_click" } else { "click" }),
                description: format!("{}click image placement {} at [{:.0}, {:.0}]", if shift { "Shift-" } else { "" }, p.id(), center.x, center.y),
                input: Input::Click { at: [center.x, center.y], shift } });
        }
        Some(json!({"id": p.id(), "screen_rect": rect_value(rect), "center_pointer_reachable": reachable,
            "world_center": [geometry.center.x, geometry.center.y], "world_size": [geometry.scaled_size().x, geometry.scaled_size().y],
            "selected": canvas.selected_placement_ids().contains(&p.id()),
            "document_content": if p.is_loaded() { "loaded" } else if matches!(p.content, crate::data::PlacementContent::Failed { .. }) { "failed" } else { "loading" },
            "source_identity_not_visible_label": p.loaded_asset().map(|a| a.filename.clone()),
            "group_id": canvas.groups().iter().find(|g| g.members.contains(&p.id())).map(|g| g.id)}))
    }).collect();
    let groups: Vec<_> = canvas
        .groups()
        .iter()
        .filter_map(|g| {
            let r = g.container.unrotated_bounds();
            let rect = egui::Rect::from_min_max(to_screen(r.min), to_screen(r.max));
            screen.intersects(rect).then(|| {
                json!({"id": g.id, "title": g.name, "screen_rect": rect_value(rect),
            "members": g.members, "selected": canvas.selected_group_ids().contains(&g.id)})
            })
        })
        .collect();
    // Generic resource drop and canvas targets. No goal-dependent pruning.
    for (name, at) in [("left", [430., 510.]), ("right", [790., 510.])] {
        for sample in 0..2 {
            options.push(OptionInput {
                id: format!("drop:{sample}:{name}"),
                description: format!("Drop sample {sample} at canvas {name}"),
                input: Input::Drop { sample, at },
            });
        }
    }
    options.push(OptionInput {
        id: "canvas_blank".into(),
        description: "Click empty canvas at bottom right to leave an editor or clear selection"
            .into(),
        input: Input::Click {
            at: [1120., 740.],
            shift: false,
        },
    });
    for (key, description) in [
        (
            "select_all",
            "Command/Ctrl+A: select all text in focused field, otherwise all canvas content",
        ),
        ("group", "Command/Ctrl+G: group selected images"),
        (
            "group_picker",
            "G: open Add to group picker for selected images",
        ),
        ("escape", "Escape"),
        ("enter", "Enter"),
        ("tab", "Tab: move keyboard focus"),
        ("undo", "Command/Ctrl+Z: undo"),
        ("redo", "Command/Ctrl+Shift+Z: redo"),
    ] {
        options.push(OptionInput {
            id: key.into(),
            description: description.into(),
            input: Input::Key(key.into()),
        });
    }
    if h.ctx.egui_wants_keyboard_input() {
        options.push(OptionInput {
            id: "replace_field_text".into(),
            description: "Replace all text in the focused editor with supplied text Reference using Select All then typing".into(),
            input: Input::Fill("Reference".into()),
        });
        options.push(OptionInput {
            id: "type_goal_name".into(),
            description:
                "Type supplied goal text Reference into the focused field, replacing selected text"
                    .into(),
            input: Input::Text("Reference".into()),
        });
    }
    for (id, description, input) in [
        ("wait", "Wait briefly for asynchronous work", Input::Wait),
        (
            "done",
            "Finish: the complete goal is visibly satisfied",
            Input::Done,
        ),
        (
            "blocked",
            "Stop: available inputs cannot make progress",
            Input::Blocked,
        ),
    ] {
        options.push(OptionInput {
            id: id.into(),
            description: description.into(),
            input,
        });
    }
    (
        json!({"viewport": {"screen_rect": rect_value(screen), "zoom": zoom, "pan": [pan.x, pan.y]},
        "controls": controls, "placements": placements, "groups": groups,
        "keyboard_captured": h.ctx.egui_wants_keyboard_input(),
        "canvas_input": format!("{:?}", app.canvas_input_policy(&h.ctx)),
        "observation_limits": "Canvas geometry and source identity are supplemental; pixels and overlap are not verified. No offscreen scene inventory."}),
        options,
    )
}

fn oracle(h: &Harness<'_, SnappyRefApp>) -> Value {
    let canvas = &h.state().canvas;
    let images = &canvas.document.placements;
    let group = canvas.groups().first();
    let names: std::collections::HashSet<_> = images
        .iter()
        .filter_map(|p| p.loaded_asset().map(|a| a.filename.clone()))
        .collect();
    let imported = images.len() == 2 && images.iter().all(|p| p.is_loaded()) && names.len() == 2;
    let grouped = canvas.groups().len() == 1
        && group.is_some_and(|g| {
            g.members.len() == 2 && images.iter().all(|p| g.members.contains(&p.id()))
        });
    let named = group.is_some_and(|g| g.name == "Reference");
    let column = group.is_some_and(|g| g.flow == crate::data::GroupFlow::Column);
    let column_geometry = images.len() == 2
        && (images[0].state.geometry.center.x - images[1].state.geometry.center.x).abs() < 0.1
        && (images[0].state.geometry.center.y - images[1].state.geometry.center.y).abs() > 1.;
    let unfocused = !h.ctx.egui_wants_keyboard_input();
    json!({"imported_once_each": imported, "group_membership": grouped, "committed_name": named,
        "column_setting": column, "column_geometry": column_geometry, "editor_unfocused": unfocused,
        "passed": imported && grouped && named && column && column_geometry && unfocused})
}

fn baseline(h: &Harness<'_, SnappyRefApp>, step: usize) -> Option<Input> {
    let (state, options) = observe(h);
    let key = |name: &str| Some(Input::Key(name.into()));
    match step {
        0 => Some(Input::Drop {
            sample: 0,
            at: [430., 510.],
        }),
        1 => Some(Input::Drop {
            sample: 1,
            at: [430., 510.],
        }),
        2 => Some(Input::Wait),
        3 => key("select_all"),
        4 => key("group"),
        5 => state["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["role"] == "TextInput")
            .and_then(|c| options.iter().find(|o| o.id == c["id"].as_str().unwrap()))
            .map(|o| o.input.clone()),
        6 => key("select_all"),
        7 => Some(Input::Text("Reference".into())),
        8 => options
            .iter()
            .find(|o| o.description.contains("labelled \"Column\""))
            .map(|o| o.input.clone()),
        9 => Some(Input::Click {
            at: [1120., 740.],
            shift: false,
        }),
        _ => Some(Input::Done),
    }
}

fn decide(client: &reqwest::blocking::Client, key: &str, request: &Value) -> Result<Value, String> {
    let response = client
        .post("https://api.typesafe.ai/v1/systemone")
        .bearer_auth(key)
        .header("Content-Type", "application/json")
        .body(request.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("TypeSafe HTTP {}", response.status()));
    }
    serde_json::from_str(&response.text().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[test]
#[ignore = "bounded manual experiment; JEV_PILOT_LIVE=1 opts into TypeSafe calls"]
fn inspect_jev_grouping_pilot_writes_artifact() {
    let live = std::env::var("JEV_PILOT_LIVE").as_deref() == Ok("1");
    let replay: Option<Value> = std::env::var("JEV_PILOT_REPLAY")
        .ok()
        .map(|p| serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap());
    assert!(!(live && replay.is_some()), "choose live or replay");
    let key = if live {
        std::env::var("TYPESAFE_API_KEY")
            .ok()
            .or_else(|| {
                std::env::var("TYPESAFE_API_KEY_FILE").ok().map(|p| {
                    fs::read_to_string(p)
                        .expect("read API key file")
                        .trim()
                        .to_owned()
                })
            })
            .filter(|s| !s.is_empty())
            .expect("set TYPESAFE_API_KEY or TYPESAFE_API_KEY_FILE")
    } else {
        String::new()
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap();
    let mut scenario = InspectScenario::new("jev-grouping-pilot");
    fs::create_dir_all(&scenario.artifact_dir).unwrap();
    let mut summaries = Vec::new();
    let mut total_tokens = 0u64;
    let trials = if replay.is_some() {
        1
    } else if live {
        6
    } else {
        3
    };
    for trial in 0..trials {
        let variant = replay
            .as_ref()
            .map_or(trial % 3, |r| r["variant"].as_u64().unwrap() as usize);
        let root = TempRoot::new("snappyref-jev-pilot");
        let library = Library::initialize(root.join("library")).unwrap();
        let mut app = test_app(Some(library.clone()));
        app.sidebar_open = false;
        app.canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_zoom: [1., 0.7, 1.3][variant],
                camera_pan_x: [0., 100., -90.][variant],
                camera_pan_y: [0., -60., 50.][variant],
                ..Default::default()
            });
        let mut h = app_full_harness(app);
        h.state_mut().import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
            library,
            h.ctx.clone(),
            None,
        ));
        let samples: Vec<_> = [
            ([78, 126, 226, 255], "blue.png"),
            ([226, 160, 78, 255], "gold.png"),
        ]
        .into_iter()
        .map(|(color, name)| {
            let path = root.join(name);
            image::RgbaImage::from_pixel(120, 90, image::Rgba(color))
                .save(&path)
                .unwrap();
            path
        })
        .collect();
        advance(&mut h);
        let started = Instant::now();
        let mut trace = Vec::<Value>::new();
        let mut outcome = "step_limit".to_owned();
        let mut api_ms = 0u128;
        let tokens_before = total_tokens;
        for step in 0..MAX_STEPS {
            let (screen, options) = observe(&h);
            scenario.capture_screen(&format!("trial-{trial:02}-step-{step:02}"), &mut h);
            let state = json!({"goal": GOAL, "resources": [{"id":0,"filename":"blue.png"},{"id":1,"filename":"gold.png"}],
                "screen": screen, "recent_inputs_oldest_first": trace.iter().skip(trace.len().saturating_sub(6)).map(|e| &e["input"]).collect::<Vec<_>>(), "steps_left": MAX_STEPS-step});
            let criteria: serde_json::Map<_, _> = options
                .iter()
                .map(|o| (o.id.clone(), json!(o.description)))
                .collect();
            let request = json!({"model":"jev-1.13.0", "state": state, "questions":{"next_input":{
                "type":"choice", "instructions":"Choose the next user input toward the whole goal. Use current controls and geometry. No fixed action sequence is provided. Consider recent inputs, avoid repeating satisfied work. All options are inputs, not guarantees of success. Choose blocked if you cannot progress.", "criteria":criteria}}});
            let mut response = Value::Null;
            let mut latency_ms = 0u128;
            let input = if let Some(replay) = &replay {
                match replay["steps"].get(step) {
                    Some(s) => serde_json::from_value(s["input"].clone()).unwrap(),
                    None => {
                        outcome = "replay_exhausted".into();
                        break;
                    }
                }
            } else if live {
                if total_tokens >= 500_000 {
                    outcome = "token_limit".into();
                    break;
                }
                let now = Instant::now();
                match decide(&client, &key, &request) {
                    Ok(r) => response = r,
                    Err(error) => {
                        outcome = format!("service_error: {error}");
                        scenario.event(
                            "service_error",
                            json!({"trial":trial,"step":step,"request":request,"error":error}),
                        );
                        break;
                    }
                }
                latency_ms = now.elapsed().as_millis();
                api_ms += latency_ms;
                total_tokens += response["usage"]["input_tokens"].as_u64().unwrap_or(0);
                let choice = response["answers"]["next_input"]["choice"].as_str();
                match options.iter().find(|o| Some(o.id.as_str()) == choice) {
                    Some(o) => o.input.clone(),
                    None => {
                        outcome = "invalid_response".into();
                        scenario.event("invalid_response", json!({"trial":trial,"step":step,"request":request,"response":response}));
                        break;
                    }
                }
            } else {
                match baseline(&h, step) {
                    Some(input) => input,
                    None => {
                        outcome = "baseline_target_missing".into();
                        break;
                    }
                }
            };
            trace.push(json!({"request": request, "response": response, "input": input, "api_ms": latency_ms}));
            fs::write(
                scenario.artifact_dir.join(format!("trial-{trial:02}.json")),
                serde_json::to_string_pretty(&json!({"variant":variant,"steps":trace})).unwrap(),
            )
            .unwrap();
            match input {
                Input::Done => {
                    outcome = "done".into();
                    break;
                }
                Input::Blocked => {
                    outcome = "blocked".into();
                    break;
                }
                _ => apply(&mut h, &input, &samples),
            }
        }
        scenario.capture_screen(&format!("trial-{trial:02}-final"), &mut h);
        let checks = oracle(&h);
        let summary = json!({"trial": trial, "variant":variant, "controller": if live {"jev"} else if replay.is_some() {"replay"} else {"script"},
            "steps":trace.len(), "outcome":outcome, "oracle":checks, "wall_ms":started.elapsed().as_millis(), "api_ms":api_ms,"input_tokens":total_tokens-tokens_before});
        scenario.event("trial", summary.clone());
        scenario.assertion(
            "controller finishes and independent oracle passes",
            outcome == "done" && checks["passed"] == true,
        );
        println!("{summary}");
        summaries.push(summary);
    }
    fs::write(
        scenario.artifact_dir.join("summary.json"),
        serde_json::to_string_pretty(&json!({"trials": summaries, "input_tokens":total_tokens}))
            .unwrap(),
    )
    .unwrap();
    let report = scenario.finish();
    println!("artifact={}", report.artifact_dir.display());
    assert!(report.passed, "pilot had failures; inspect recorded trials");
}
