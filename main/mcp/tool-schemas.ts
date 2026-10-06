export type McpToolDefinition = {
  name: string;
  description: string;
  inputSchema: {
    type: "object";
    properties: Record<string, unknown>;
    required?: string[];
    additionalProperties: false;
  };
};

export const MCP_TOOLS: McpToolDefinition[] = [
  {
    name: "get_project_state",
    description:
      "Inspect whether the editor is active and read the canvas, timeline, elements, and animations before making changes. This is the only tool that works without an open project; positions and dimensions use canvas pixels with origin at the top-left, and all times are milliseconds.",
    inputSchema: {
      type: "object",
      properties: {},
      additionalProperties: false,
    },
  },
  {
    name: "add_text",
    description:
      "Add a single plain-text element; use add_math for typeset LaTeX or import_manim_scene to build a whole scene. Font size defaults to 32 pixels and font weight to 400; omitted x/y coordinates centre the element on the corresponding canvas axis, and omitted color uses the app's default text color. Coordinates are canvas pixels with origin at the top-left; an open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        text: { type: "string", description: "Plain text to display in the new element." },
        fontSize: { type: "number", default: 32, description: "Font size in pixels; defaults to 32 when omitted." },
        fontWeight: { type: "number", default: 400, description: "Numeric font weight; defaults to 400 (normal) when omitted." },
        x: { type: "number", description: "Horizontal position in canvas pixels from the left edge; omitted means centred horizontally." },
        y: { type: "number", description: "Vertical position in canvas pixels from the top edge; omitted means centred vertically." },
        color: { type: "string", description: "Text color string understood by the app; omitted uses the app's default text color." },
      },
      required: ["text"],
      additionalProperties: false,
    },
  },
  {
    name: "add_math",
    description:
      "Add a single LaTeX fragment without surrounding $ delimiters, rendered through the existing KaTeX rasteriser as a mafs-type element. Use add_text for plain text or import_manim_scene for a whole scene; omitted x/y coordinates centre the element on the corresponding canvas axis, and omitted color uses the app's default math color. Coordinates are canvas pixels with origin at the top-left; an open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        latex: { type: "string", description: "LaTeX fragment to typeset, without surrounding $ delimiters." },
        x: { type: "number", description: "Horizontal position in canvas pixels from the left edge; omitted means centred horizontally." },
        y: { type: "number", description: "Vertical position in canvas pixels from the top edge; omitted means centred vertically." },
        color: { type: "string", description: "Math color string understood by the app; omitted uses the app's default math color." },
      },
      required: ["latex"],
      additionalProperties: false,
    },
  },
  {
    name: "import_manim_scene",
    description:
      "Build a whole scene at once from a Manim Community Python script using the existing parseManimScene → translateManimScene pipeline; prefer this for coordinated elements and animations over repeated single-element calls. Unsupported constructs produce warnings rather than failing the import; the result reports element and animation counts, duration in milliseconds, and warnings. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        script: { type: "string", description: "Manim Community Python scene source for the existing parser and translator; unsupported constructs are reported as warnings." },
      },
      required: ["script"],
      additionalProperties: false,
    },
  },
  {
    name: "add_animation",
    description:
      "Attach one supported animation to an existing element; use update_element to change its static placement or time frame instead. Duration is in milliseconds; direction applies only to slideIn/slideOut, and omitting it uses the app's default slide direction. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        targetId: { type: "string", description: "ID of the existing element to animate, available from get_project_state or an element creation result." },
        type: { type: "string", enum: ["fadeIn", "fadeOut", "slideIn", "slideOut", "breathe", "mafsReveal"], description: "Supported animation kind to attach to the target element." },
        durationMs: { type: "number", description: "Animation duration in milliseconds." },
        direction: { type: "string", enum: ["left", "right", "top", "bottom"], description: "Slide direction, applicable only to slideIn/slideOut; omitted uses the app's default slide direction." },
      },
      required: ["targetId", "type", "durationMs"],
      additionalProperties: false,
    },
  },
  {
    name: "update_element",
    description:
      "Update an existing element's placement or visible time frame; use add_animation to attach motion or effects. Only supplied fields change: omitted placement/timeFrame objects and their omitted fields retain their current values; coordinates and dimensions use canvas pixels with origin at the top-left, and timeline times are milliseconds. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        id: { type: "string", description: "ID of the existing element to update." },
        placement: {
          type: "object",
          description: "Partial placement update; omitted fields retain their current values.",
          properties: {
            x: { type: "number", description: "Horizontal position in canvas pixels from the left edge; omitted retains the current position." },
            y: { type: "number", description: "Vertical position in canvas pixels from the top edge; omitted retains the current position." },
            width: { type: "number", description: "Element width in canvas pixels; omitted retains the current width." },
            height: { type: "number", description: "Element height in canvas pixels; omitted retains the current height." },
            rotation: { type: "number", description: "Element rotation in degrees; omitted retains the current rotation." },
            scaleX: { type: "number", description: "Dimensionless horizontal scale factor; omitted retains the current scale." },
            scaleY: { type: "number", description: "Dimensionless vertical scale factor; omitted retains the current scale." },
          },
          additionalProperties: false,
        },
        timeFrame: {
          type: "object",
          description: "Partial visible time-frame update; omitted fields retain their current values.",
          properties: {
            start: { type: "number", description: "Start time on the project timeline in milliseconds; omitted retains the current start." },
            end: { type: "number", description: "End time on the project timeline in milliseconds; omitted retains the current end." },
          },
          additionalProperties: false,
        },
      },
      required: ["id"],
      additionalProperties: false,
    },
  },
  {
    name: "remove_element",
    description:
      "Remove an element by ID and report whether it was removed; use update_element to modify an element you want to keep. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        id: { type: "string", description: "ID of the element to remove." },
      },
      required: ["id"],
      additionalProperties: false,
    },
  },
  {
    name: "set_canvas",
    description:
      "Change project-wide canvas dimensions, background color, or timeline duration; use update_element for an individual element. Dimensions are canvas pixels and maxTimeMs is milliseconds; omitted properties retain their current values. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        width: { type: "number", description: "Canvas width in pixels; omitted retains the current width." },
        height: { type: "number", description: "Canvas height in pixels; omitted retains the current height." },
        backgroundColor: { type: "string", description: "Canvas background color string understood by the app; omitted retains the current color." },
        maxTimeMs: { type: "number", description: "Project timeline duration in milliseconds; omitted retains the current duration." },
      },
      additionalProperties: false,
    },
  },
  {
    name: "seek",
    description:
      "Move the playhead to a timeline time in milliseconds to inspect a particular moment; use set_playing to start or pause playback. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        timeMs: { type: "number", description: "Requested playhead position on the project timeline in milliseconds." },
      },
      required: ["timeMs"],
      additionalProperties: false,
    },
  },
  {
    name: "set_playing",
    description:
      "Start or pause timeline playback; use seek to move the playhead to a specific time in milliseconds. An open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        playing: { type: "boolean", description: "True starts playback; false pauses playback." },
      },
      required: ["playing"],
      additionalProperties: false,
    },
  },
  {
    name: "save_project",
    description:
      "Save the editable project to a supplied file path, or overwrite the currently-open file when path is omitted. Omitting path fails if the project has never been saved; an open project is required, otherwise this tool fails with a clear error.",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string", description: "Destination project file path; omitted overwrites the currently-open file and errors if the project has never been saved." },
      },
      additionalProperties: false,
    },
  },
  {
    name: "add_media",
    description:
      "Import an image, video, or audio file from an absolute path on the machine running AniMathIO into the open project. Selects the matching resource panel and waits for decoded media before adding timeline elements. Returns the created element IDs; video creates both video and audio elements. Common image formats, MP4/WebM/MOV/M4V video, and MP3/WAV/OGG/M4A/AAC/FLAC audio are accepted by extension; decoding depends on Electron's codecs. Missing, unsupported, or undecodable files fail with a clear error.",
    inputSchema: {
      type: "object",
      properties: { path: { type: "string", description: "Absolute local filesystem path to an existing media file." } },
      required: ["path"],
      additionalProperties: false,
    },
  },
  {
    name: "export_video",
    description:
      "Render the open project to an absolute .mp4 or .webm file path, overwriting an existing file. Requires an existing parent directory. Format defaults to the path extension; an explicit format must match it. Recording runs in real time: a 30-second project takes at least 30 seconds, plus conversion and disk writing. MP4 conversion downloads the FFmpeg core if needed. Returns only after the file is written; allow a long client timeout and avoid editing the project during export.",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string", description: "Absolute destination path ending in .mp4 or .webm." },
        format: { type: "string", enum: ["mp4", "webm"], description: "Optional output format; inferred from path when omitted, must match its extension." },
      },
      required: ["path"],
      additionalProperties: false,
    },
  },
];

export const MCP_TOOL_NAMES = MCP_TOOLS.map(t => t.name);
export type McpToolName = (typeof MCP_TOOLS)[number]["name"];
