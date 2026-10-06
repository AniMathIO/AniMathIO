import { describe, it, expect } from 'vitest';
import { MCP_TOOLS } from '../../../main/mcp/tool-schemas';

describe('MCP_TOOLS', () => {
  it('has exactly 13 entries', () => {
    expect(MCP_TOOLS).toHaveLength(13);
  });

  it('has exactly the expected tool names', () => {
    const expectedNames = [
      'get_project_state',
      'add_text',
      'add_math',
      'import_manim_scene',
      'add_animation',
      'update_element',
      'remove_element',
      'set_canvas',
      'seek',
      'set_playing',
      'save_project',
      'add_media',
      'export_video'
    ];
    const actualNames = MCP_TOOLS.map(tool => tool.name);
    expect(actualNames).toEqual(expectedNames);
  });

  it('has unique tool names', () => {
    const names = MCP_TOOLS.map(tool => tool.name);
    const uniqueNames = new Set(names);
    expect(uniqueNames.size).toBe(names.length);
  });

  it('every tool has a non-empty description of at least 40 characters', () => {
    for (const tool of MCP_TOOLS) {
      expect(tool.description).toBeTruthy();
      expect(tool.description.length).toBeGreaterThanOrEqual(40);
    }
  });

  it('every inputSchema has type === "object" and additionalProperties === false', () => {
    for (const tool of MCP_TOOLS) {
      expect(tool.inputSchema.type).toBe('object');
      expect(tool.inputSchema.additionalProperties).toBe(false);
    }
  });

  it('for every tool, each name listed in inputSchema.required is also a key in inputSchema.properties', () => {
    for (const tool of MCP_TOOLS) {
      if (tool.inputSchema.required) {
        for (const requiredProp of tool.inputSchema.required) {
          expect(tool.inputSchema.properties).toHaveProperty(requiredProp);
        }
      }
    }
  });

  it('add_text requires "text"', () => {
    const addTextTool = MCP_TOOLS.find(tool => tool.name === 'add_text');
    expect(addTextTool).toBeDefined();
    expect(addTextTool!.inputSchema.required).toContain('text');
  });

  it('add_math requires "latex"', () => {
    const addMathTool = MCP_TOOLS.find(tool => tool.name === 'add_math');
    expect(addMathTool).toBeDefined();
    expect(addMathTool!.inputSchema.required).toContain('latex');
  });

  it('import_manim_scene requires "script"', () => {
    const importManimTool = MCP_TOOLS.find(tool => tool.name === 'import_manim_scene');
    expect(importManimTool).toBeDefined();
    expect(importManimTool!.inputSchema.required).toContain('script');
  });

  it('add_animation requires targetId, type and durationMs', () => {
    const addAnimationTool = MCP_TOOLS.find(tool => tool.name === 'add_animation');
    expect(addAnimationTool).toBeDefined();
    expect(addAnimationTool!.inputSchema.required).toContain('targetId');
    expect(addAnimationTool!.inputSchema.required).toContain('type');
    expect(addAnimationTool!.inputSchema.required).toContain('durationMs');
  });

  it('get_project_state has no required arguments', () => {
    const getProjectStateTool = MCP_TOOLS.find(tool => tool.name === 'get_project_state');
    expect(getProjectStateTool).toBeDefined();
    expect(getProjectStateTool!.inputSchema.required).toBeUndefined();
  });
});
