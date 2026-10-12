//! The tool catalogue `--mcp` advertises. It is static, so a client can list
//! tools before the app runs. `crate::app::control` runs each tool.

use serde_json::{Value, json};

/// Sent once at `initialize`. Rules that hold for every tool live here so
/// tool descriptions stay short.
pub(super) const INSTRUCTIONS: &str = "\
SnappyRef is a reference board app. A board is an unbounded canvas of images, labels, \
rectangles, arrows and lines. Groups lay out images, labels and rectangles together.

Read the open board with get_board, then change it with edit_board. One edit_board call \
is one undo step, so batch related edits.

Conventions:
- Ids are stable numbers. Images, annotations, groups, boards and library assets each have one.
- Coordinates are world units. x grows right, y grows down. x and y are an item's top-left \
corner before rotation. Rotation is degrees clockwise about the item's center.
- Items are listed back to front.
- An image keeps its aspect ratio. Set width or height, never both.
- A label's size follows its text. Arrows and lines do not stay attached to items they point at.
- A group with flow wrap, row or column positions its members. Set x and y on the group, \
not on its members. Flow free leaves members where you put them.";

struct Tool {
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &'static [&'static str],
    read_only: bool,
    destructive: bool,
}

pub(super) fn catalogue() -> Value {
    Value::Array(tools().into_iter().map(Tool::into_json).collect())
}

impl Tool {
    fn into_json(self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": {
                "type": "object",
                "properties": self.properties,
                "required": self.required,
                "additionalProperties": false,
            },
            "annotations": {
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
            },
        })
    }
}

fn tools() -> Vec<Tool> {
    let read = |name, description, properties, required| Tool {
        name,
        description,
        properties,
        required,
        read_only: true,
        destructive: false,
    };
    let write = |name, description, properties, required| Tool {
        name,
        description,
        properties,
        required,
        read_only: false,
        destructive: false,
    };
    let destroy = |name, description, properties, required| Tool {
        name,
        description,
        properties,
        required,
        read_only: false,
        destructive: true,
    };
    vec![
        read(
            "get_board",
            "Returns a board: its items back to front with geometry and properties, and its \
             groups. For the open board it also returns the selection and the visible area. \
             Omit `board` for the open board.",
            json!({"board": {"type": "integer", "description": "Board id. Reads without opening it."}}),
            &[],
        ),
        read(
            "list_boards",
            "Lists every board with its id, name and image count, and says which one is open.",
            json!({}),
            &[],
        ),
        write(
            "edit_board",
            "Applies a list of edits to the open board as one undo step. Returns the items \
             and groups the edits touched, with their final geometry.\n\
             Each edit has an `action`:\n\
             - add: `type` image, label, rectangle, arrow or line, plus x and y (x1, y1, x2, \
             y2 for arrow and line). An image needs `asset`, an id from search_library or \
             from an image on any board. A label needs `text`. A rectangle needs width and \
             height. `group` puts the new item in a group. `ref` names it.\n\
             - set: `id` or `ids`, plus the properties to change. dx and dy move by an offset.\n\
             - delete: `ids`. A group id deletes the group and its members.\n\
             - group: `ids` of images, labels and rectangles. Optional name, flow, gap, \
             padding, ref.\n\
             - ungroup: `ids` of groups. The members stay.\n\
             - add_to_group: `group`, `ids`.\n\
             - remove_from_group: `ids`.\n\
             - arrange: `ids` of images, `layout` row, column or compact, optional gap.\n\
             - order: `ids`, `to` front or back.\n\
             A later edit in the same call can refer to an added item or new group as \
             \"$name\", where name is its `ref`.\n\
             Properties by type. image: x, y, width or height, rotation. label: x, y, text, \
             size, color. rectangle: x, y, width, height, rotation, color, fill, stroke. \
             arrow and line: x1, y1, x2, y2, color, stroke. group: x, y, width, height, name, \
             flow, gap, padding.\n\
             If an edit fails, nothing in the call is applied.",
            json!({
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "action": {"enum": ["add", "set", "delete", "group", "ungroup", "add_to_group", "remove_from_group", "arrange", "order"]},
                            "type": {"enum": ["image", "label", "rectangle", "arrow", "line"]},
                            "ref": {"type": "string", "description": "Name for the item or group this edit creates."},
                            "id": {"type": ["integer", "string"]},
                            "ids": {"type": "array", "items": {"type": ["integer", "string"]}, "description": "Item or group ids, or \"$name\" refs."},
                            "group": {"type": ["integer", "string"]},
                            "asset": {"type": "integer"},
                            "x": {"type": "number"},
                            "y": {"type": "number"},
                            "width": {"type": "number"},
                            "height": {"type": "number"},
                            "rotation": {"type": "number", "description": "Degrees clockwise."},
                            "dx": {"type": "number"},
                            "dy": {"type": "number"},
                            "x1": {"type": "number"},
                            "y1": {"type": "number"},
                            "x2": {"type": "number", "description": "An arrow points at x2, y2."},
                            "y2": {"type": "number"},
                            "text": {"type": "string"},
                            "size": {"enum": ["s", "m", "l", "xl", "xxl"], "description": "Label text size."},
                            "stroke": {"enum": ["s", "m", "l", "xl", "xxl"], "description": "Line thickness."},
                            "color": {"enum": ["default", "coral", "gold", "mint", "sky"]},
                            "fill": {"enum": ["none", "tint", "solid"]},
                            "name": {"type": "string"},
                            "flow": {"enum": ["wrap", "row", "column", "free"]},
                            "gap": {"type": "number", "description": "Space between group members or arranged images. Defaults to the placement gap setting."},
                            "padding": {"type": "number", "description": "Space inside a group's frame. Defaults to the placement gap setting."},
                            "layout": {"enum": ["row", "column", "compact"]},
                            "to": {"enum": ["front", "back"]},
                        },
                        "required": ["action"],
                        "additionalProperties": false,
                    },
                },
            }),
            &["edits"],
        ),
        write(
            "add_images",
            "Imports image files or image URLs into the library and places them on the open \
             board. Waits until they load, then returns the new items. One undo step. Without \
             x and y the images land in the visible area, packed together. The new images \
             become the selection, as they do when the user imports. To place an image that \
             is already in the library, use edit_board instead.",
            json!({
                "sources": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "string"},
                    "description": "Absolute file paths or http(s) URLs.",
                },
                "x": {"type": "number", "description": "Center of the placed images."},
                "y": {"type": "number"},
                "group": {"type": "integer", "description": "Group id that receives the images."},
            }),
            &["sources"],
        ),
        read(
            "search_library",
            "Finds images in the library by meaning. Needs the search model, see get_settings. \
             Give a text `query`, an asset id in `similar_to`, or both. Returns assets, best \
             match first. Place one with edit_board.",
            json!({
                "query": {"type": "string"},
                "similar_to": {"type": "integer", "description": "Asset id."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20."},
            }),
            &[],
        ),
        write(
            "create_board",
            "Creates a board and opens it. `pureref` imports a PureRef .pur file as the new board.",
            json!({
                "name": {"type": "string"},
                "pureref": {"type": "string", "description": "Absolute path of a .pur file."},
            }),
            &[],
        ),
        write(
            "open_board",
            "Opens a board in the app.",
            json!({"board": {"type": "integer"}}),
            &["board"],
        ),
        write(
            "rename_board",
            "Renames a board.",
            json!({"board": {"type": "integer"}, "name": {"type": "string"}}),
            &["board", "name"],
        ),
        destroy(
            "delete_board",
            "Deletes a board. Its images stay in the library. Undo restores the board.",
            json!({"board": {"type": "integer"}}),
            &["board"],
        ),
        write(
            "undo",
            "Undoes the last edit on the open board, the same as the user pressing undo.",
            json!({}),
            &[],
        ),
        write("redo", "Redoes the edit undo reverted.", json!({}), &[]),
        write(
            "set_view",
            "Moves the camera or changes the canvas background. `fit` frames the whole board \
             when true, or the listed items. Otherwise x and y are the top-left corner of the \
             visible area, as get_board reports it in `view`, and zoom is screen points per \
             world unit.",
            json!({
                "fit": {"type": ["boolean", "array"], "items": {"type": "integer"}},
                "x": {"type": "number"},
                "y": {"type": "number"},
                "zoom": {"type": "number"},
                "background": {"enum": ["none", "grid", "dots"]},
            }),
            &[],
        ),
        write(
            "select",
            "Selects items or groups in the app so the user sees them highlighted. An empty \
             list clears the selection. Editing does not need a selection.",
            json!({"ids": {"type": "array", "items": {"type": "integer"}, "description": "Item or group ids."}}),
            &["ids"],
        ),
        read(
            "screenshot",
            "Returns a PNG of the app window as the user would see it. It also answers while \
             the window is minimized or behind other windows.",
            json!({}),
            &[],
        ),
        write(
            "export_board",
            "Saves the open board as a new PureRef .pur file. It does not replace a file.",
            json!({"path": {"type": "string", "description": "Absolute path ending in .pur that no file has yet."}}),
            &["path"],
        ),
        read(
            "get_settings",
            "Returns the app settings, the app version and the state of the search model.",
            json!({}),
            &[],
        ),
        write(
            "set_settings",
            "Changes app settings. Only the given ones change.",
            json!({
                "placement_gap": {"type": "number", "description": "Space between images the app lays out, 0 to 128."},
                "rotation_snap": {"type": "number", "description": "Degrees, 0 to 180. 0 turns snapping off."},
                "pointer": {"enum": ["mouse", "trackpad"]},
                "always_on_top": {"type": "boolean"},
                "sidebar_open": {"type": "boolean"},
            }),
            &[],
        ),
        destroy(
            "remove_from_library",
            "Deletes images from the library and from every board. There is no undo. Unless \
             AI control is set to full access, the user confirms in the app first.",
            json!({"assets": {"type": "array", "minItems": 1, "items": {"type": "integer"}, "description": "Asset ids."}}),
            &["assets"],
        ),
        destroy(
            "search_model",
            "Installs or removes the model search_library needs. Install downloads it in the \
             background, and get_settings reports progress. Unless AI control is set to full \
             access, the user confirms in the app first. With full access, remove during a \
             download cancels it.",
            json!({"action": {"enum": ["install", "remove"]}}),
            &["action"],
        ),
    ]
}

#[cfg(test)]
pub(crate) fn tool_names() -> Vec<&'static str> {
    tools().into_iter().map(|tool| tool.name).collect()
}
