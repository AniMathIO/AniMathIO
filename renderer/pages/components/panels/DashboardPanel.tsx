"use client";
import React, { useEffect, useState } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Image from "next/image";
import {
  getAllProjects,
  deleteProjectFromHistory,
  addProjectToHistory,
  ProjectHistoryItem,
} from "@/utils";
import { PiUploadSimpleBold } from "react-icons/pi";
import { IoTrashOutline } from "react-icons/io5";
import { FiSearch } from "react-icons/fi";
import { IoAddCircleOutline } from "react-icons/io5";
import { HiOutlineGlobe } from "react-icons/hi";
import { HiOutlineBookOpen } from "react-icons/hi";
import { FaDiscord } from "react-icons/fa";

// Type declaration is in SettingsModal.tsx

type SortOption = "lastUpdate" | "fileName";

const DashboardPanel = observer(() => {
  const state = React.useContext(StateContext);
  const [projects, setProjects] = useState<ProjectHistoryItem[]>([]);
  const [filteredProjects, setFilteredProjects] = useState<
    ProjectHistoryItem[]
  >([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [sortBy, setSortBy] = useState<SortOption>("lastUpdate");
  const [sortAscending, setSortAscending] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  const loadProjects = async () => {
    setIsLoading(true);
    try {
      const allProjects = await getAllProjects();
      setProjects(allProjects);
      applyFiltersAndSort(allProjects);
    } catch (error) {
      console.error("Error loading projects:", error);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadProjects();
  }, []);

  const applyFiltersAndSort = (projectsToFilter: ProjectHistoryItem[]) => {
    let filtered = [...projectsToFilter];

    // Apply search filter
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (project) =>
          project.fileName.toLowerCase().includes(query) ||
          project.filePath.toLowerCase().includes(query)
      );
    }

    // Apply sorting
    filtered.sort((a, b) => {
      let comparison = 0;
      if (sortBy === "lastUpdate") {
        comparison = a.lastUpdate - b.lastUpdate;
      } else if (sortBy === "fileName") {
        comparison = a.fileName.localeCompare(b.fileName);
      }
      return sortAscending ? comparison : -comparison;
    });

    setFilteredProjects(filtered);
  };

  useEffect(() => {
    applyFiltersAndSort(projects);
  }, [searchQuery, sortBy, sortAscending, projects]);

  const handleNewProject = async () => {
    try {
      // Create an empty project (reset state)
      // Reset the editor to default state
      state.editorElements = [];
      state.videos = [];
      state.images = [];
      state.audios = [];
      state.backgroundColor = "#111111";
      state.maxTime = 30 * 1000;
      state.canvas_width = 800;
      state.canvas_height = 600;
      state.animations = [];
      state.currentKeyFrame = 0;
      state.selectedElement = null;
      if (state.canvas) {
        state.setCanvasSize(800, 600);
      }

      // Use Electron IPC to save the empty project and get full path
      if (window.electron && window.electron.saveProjectFile) {
        try {
          // Serialize empty project
          const buffer = await state.serialize();
          const fileData = Array.from(new Uint8Array(buffer));
          const suggestedName = `project-${new Date().toISOString()}.animathio`;

          const result = await window.electron.saveProjectFile(
            fileData,
            suggestedName
          );

          if (!result.success) {
            if (result.error === "File save cancelled") {
              return; // User cancelled, don't activate editor
            }
            throw new Error(result.error || "Failed to save new project");
          }

          // Store file information with full path
          if (result.fileName) {
            state.setCurrentProjectFileName(result.fileName);
          }
          if (result.filePath) {
            state.setCurrentProjectFilePath(result.filePath);
          }
          state.setCurrentProjectFileHandle(null);

          // Add to history with full path
          if (result.filePath && result.fileName) {
            await addProjectToHistory(result.filePath, result.fileName);
          }

          // Activate editor
          state.setEditorActive(true);
          state.setSelectedMenuOption("Videos");

          // Reload projects list
          await loadProjects();
          return;
        } catch (error: any) {
          // User cancelled or error occurred
          if (error.message && error.message.includes("cancelled")) {
            return; // User cancelled, do nothing
          }
          console.error("Error creating new project:", error);
          alert("Failed to create new project.");
          return;
        }
      }

      // Fallback: Use File System Access API
      if ("showSaveFilePicker" in window) {
        try {
          // @ts-ignore
          const fileHandle = await window.showSaveFilePicker({
            suggestedName: `project-${new Date().toISOString()}.animathio`,
            types: [
              {
                description: "Animathio Project File",
                accept: { "application/octet-stream": [".animathio"] },
              },
            ],
          });

          // Serialize and save empty project
          const buffer = await state.serialize();
          const blob = new Blob([buffer], { type: "application/octet-stream" });
          // @ts-ignore
          const writable = await fileHandle.createWritable();
          await writable.write(blob);
          await writable.close();

          // Get the file name (can't get full path from FileSystemFileHandle)
          const fileName = fileHandle.name;
          const filePath = fileName; // Limitation: can't get full path

          // Add to history
          await addProjectToHistory(filePath, fileName);

          // Set current project path and file handle
          state.setCurrentProjectFilePath(filePath);
          state.setCurrentProjectFileName(fileName);
          // @ts-ignore
          state.setCurrentProjectFileHandle(fileHandle);

          // Activate editor
          state.setEditorActive(true);
          state.setSelectedMenuOption("Videos");

          // Reload projects list
          await loadProjects();
          return;
        } catch (error: any) {
          // User cancelled or error occurred
          if (error.name !== "AbortError") {
            console.error("Error creating new project:", error);
            alert("Failed to create new project.");
          }
          return;
        }
      } else {
        // Fallback: use file input
        const input = document.createElement("input");
        input.type = "file";
        input.accept = ".animathio";
        input.style.display = "none";
        input.onchange = async (e) => {
          const file = (e.target as HTMLInputElement).files?.[0];
          if (file) {
            // Create empty project
            state.editorElements = [];
            state.videos = [];
            state.images = [];
            state.audios = [];
            state.backgroundColor = "#111111";
            state.maxTime = 30 * 1000;
            state.canvas_width = 800;
            state.canvas_height = 600;
            state.animations = [];
            state.currentKeyFrame = 0;
            state.selectedElement = null;
            if (state.canvas) {
              state.setCanvasSize(800, 600);
            }

            // Add to history
            await addProjectToHistory(file.name, file.name);
            state.setCurrentProjectFilePath(file.name);
            state.setCurrentProjectFileName(file.name);
            state.setCurrentProjectFileHandle(null); // Can't store file handle from file input
            state.setEditorActive(true);
            state.setSelectedMenuOption("Videos");
            await loadProjects();
          }
        };
        document.body.appendChild(input);
        input.click();
        document.body.removeChild(input);
      }
    } catch (error) {
      console.error("Error creating new project:", error);
      alert("Failed to create new project.");
    }
  };

  const handleOpenProject = async () => {
    try {
      // Use Electron IPC to open file with dialog (for files not in history)
      if (window.electron && window.electron.openProjectFile) {
        try {
          const result = await window.electron.openProjectFile();

          if (!result.success) {
            if (result.error === "File selection cancelled") {
              return; // User cancelled, do nothing
            }
            throw new Error(result.error || "Failed to open file");
          }

          // Convert array back to ArrayBuffer
          const buffer = new Uint8Array(result.data || []).buffer;

          // Deserialize the project
          await state.deserialize(buffer);

          // Store file information
          if (result.fileName) {
            state.setCurrentProjectFileName(result.fileName);
          }
          if (result.filePath) {
            state.setCurrentProjectFilePath(result.filePath);
          }
          state.setCurrentProjectFileHandle(null);

          // Add to project history
          if (result.filePath && result.fileName) {
            await addProjectToHistory(result.filePath, result.fileName);
          }

          // Activate editor
          state.setEditorActive(true);
          state.setSelectedMenuOption("Videos");

          // Reload projects list
          await loadProjects();
          return;
        } catch (error) {
          console.error("Error opening project via IPC:", error);
          alert(
            `Failed to open project: ${
              error instanceof Error ? error.message : "Unknown error"
            }`
          );
          return;
        }
      }

      // Fallback: Use file input
      const input = document.createElement("input");
      input.type = "file";
      input.accept = ".animathio";
      input.style.display = "none";

      input.onchange = async (e) => {
        const file = (e.target as HTMLInputElement).files?.[0];
        if (file) {
          const reader = new FileReader();

          reader.onload = async (event) => {
            try {
              const arrayBuffer = event.target?.result as ArrayBuffer;

              // Deserialize the project
              await state.deserialize(arrayBuffer);

              // Store file information
              state.setCurrentProjectFileName(file.name);
              state.setCurrentProjectFilePath(file.name);
              state.setCurrentProjectFileHandle(null);

              // Add to project history
              await addProjectToHistory(file.name, file.name);

              // Activate editor
              state.setEditorActive(true);
              state.setSelectedMenuOption("Videos");

              // Reload projects list
              await loadProjects();
            } catch (error) {
              console.error("Error loading project:", error);
              alert(
                "Failed to load project. The file might be corrupted or incompatible."
              );
            }
          };

          reader.onerror = () => {
            console.error("File reading error");
            alert("Failed to read the file. Please try again.");
          };

          reader.readAsArrayBuffer(file);
        }
      };

      document.body.appendChild(input);
      input.click();
      document.body.removeChild(input);
    } catch (error) {
      console.error("Error opening project:", error);
      alert("Failed to open project.");
    }
  };

  const handleLoadProject = async (project: ProjectHistoryItem) => {
    try {
      // Use Electron IPC to read the file directly by path (no dialog)
      if (window.electron && window.electron.readProjectFile) {
        try {
          const result = await window.electron.readProjectFile(
            project.filePath
          );

          if (!result.success) {
            // File not found - show error
            alert(
              `Failed to open project: ${result.error || "File not found"}`
            );
            return;
          }

          // Convert array back to ArrayBuffer
          const buffer = new Uint8Array(result.data || []).buffer;

          // Deserialize the project
          await state.deserialize(buffer);

          // Store file information
          if (result.fileName) {
            state.setCurrentProjectFileName(result.fileName);
          }
          if (result.filePath) {
            state.setCurrentProjectFilePath(result.filePath);
          }
          state.setCurrentProjectFileHandle(null); // File handle not available through IPC

          // Update project history with current timestamp
          if (result.filePath && result.fileName) {
            await addProjectToHistory(result.filePath, result.fileName);
          }

          // Activate editor
          state.setEditorActive(true);
          state.setSelectedMenuOption("Videos");

          // Reload projects list
          await loadProjects();
          return;
        } catch (error) {
          console.error("Error loading project via IPC:", error);
          alert(
            `Failed to load project: ${
              error instanceof Error ? error.message : "Unknown error"
            }`
          );
          return;
        }
      }

      // Fallback: Use file input if Electron IPC is not available
      const input = document.createElement("input");
      input.type = "file";
      input.accept = ".animathio";
      input.style.display = "none";

      input.onchange = async (e) => {
        const file = (e.target as HTMLInputElement).files?.[0];
        if (file) {
          const reader = new FileReader();

          reader.onload = async (event) => {
            try {
              const arrayBuffer = event.target?.result as ArrayBuffer;
              const buffer = new Uint8Array(arrayBuffer);

              // Deserialize the project
              await state.deserialize(arrayBuffer);

              // Store file information
              state.setCurrentProjectFileName(file.name);
              state.setCurrentProjectFilePath(project.filePath);
              state.setCurrentProjectFileHandle(null);

              // Update project history with current timestamp
              await addProjectToHistory(project.filePath, file.name);

              // Activate editor
              state.setEditorActive(true);
              state.setSelectedMenuOption("Videos");

              // Reload projects list
              await loadProjects();
            } catch (error) {
              console.error("Error loading project:", error);
              alert(
                "Failed to load project. The file might be corrupted or incompatible."
              );
            }
          };

          reader.onerror = () => {
            console.error("File reading error");
            alert("Failed to read the file. Please try again.");
          };

          reader.readAsArrayBuffer(file);
        }
      };

      document.body.appendChild(input);
      input.click();
      document.body.removeChild(input);
    } catch (error) {
      console.error("Error loading project:", error);
      alert("Failed to load project.");
    }
  };

  const handleDeleteProject = async (id: number) => {
    if (confirm("Are you sure you want to remove this project from history?")) {
      try {
        await deleteProjectFromHistory(id);
        await loadProjects();
      } catch (error) {
        console.error("Error deleting project:", error);
        alert("Failed to delete project from history.");
      }
    }
  };

  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp);
    return date.toLocaleString();
  };

  const handleSort = (newSortBy: SortOption) => {
    if (sortBy === newSortBy) {
      setSortAscending(!sortAscending);
    } else {
      setSortBy(newSortBy);
      setSortAscending(false);
    }
  };

  return (
    <div className="h-full w-full bg-slate-200 dark:bg-gray-800 text-slate-900 dark:text-white overflow-hidden">
      <div className="max-w-5xl mx-auto h-full px-6 pt-4 flex flex-col gap-4">
        {/* Header */}
        <div className="flex flex-col gap-3 border border-slate-200 dark:border-gray-700 rounded-2xl bg-white dark:bg-gray-900 shadow-sm p-5">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="h-12 w-12 rounded-xl border border-slate-200 dark:border-white/10 bg-white dark:bg-black/30 flex items-center justify-center">
                <Image
                  src="/images/AniMathIO.png"
                  alt="AniMathIO logo"
                  width={28}
                  height={28}
                  className="object-contain"
                />
              </div>
              <div className="leading-tight">
                <p className="text-[0.95rem] font-semibold text-slate-600 dark:text-gray-200">
                  AniMathIO
                </p>
                <h1 className="text-3xl font-semibold text-slate-900 dark:text-white mt-1">
                  Dashboard
                </h1>
              </div>
            </div>
            <div className="flex items-center gap-3">
              <button
                onClick={() => {
                  if (window.electron && window.electron.openExternalUrl) {
                    window.electron.openExternalUrl("https://animathio.com");
                  } else {
                    window.open("https://animathio.com", "_blank");
                  }
                }}
                className="p-2 rounded-lg hover:bg-slate-100 dark:hover:bg-gray-800 transition-colors"
                title="Website"
              >
                <HiOutlineGlobe className="h-5 w-5 text-slate-600 dark:text-gray-300" />
              </button>
              <button
                onClick={() => {
                  if (window.electron && window.electron.openExternalUrl) {
                    window.electron.openExternalUrl("https://docs.animathio.com/");
                  } else {
                    window.open("https://docs.animathio.com/", "_blank");
                  }
                }}
                className="p-2 rounded-lg hover:bg-slate-100 dark:hover:bg-gray-800 transition-colors"
                title="Documentation"
              >
                <HiOutlineBookOpen className="h-5 w-5 text-slate-600 dark:text-gray-300" />
              </button>
              <button
                onClick={() => {
                  if (window.electron && window.electron.openExternalUrl) {
                    window.electron.openExternalUrl("https://discord.com/invite/cZMTYSAHRX");
                  } else {
                    window.open("https://discord.com/invite/cZMTYSAHRX", "_blank");
                  }
                }}
                className="p-2 rounded-lg hover:bg-slate-100 dark:hover:bg-gray-800 transition-colors"
                title="Discord"
              >
                <FaDiscord className="h-5 w-5 text-slate-600 dark:text-gray-300" />
              </button>
            </div>
          </div>
        </div>

        {/* Projects list */}
        <div className="bg-white dark:bg-gray-900 rounded-2xl border border-slate-200 dark:border-gray-700 shadow-sm p-4 flex flex-col flex-1 min-h-0">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between mb-4">
            <div>
              <h2 className="text-lg font-semibold">Recent Projects</h2>
              <p className="text-xs text-slate-500 dark:text-gray-400">
                Create, or open a recent AniMathIO project.
              </p>
            </div>
            <div className="flex gap-2 w-full sm:w-auto">
              <button
                onClick={handleNewProject}
                className="flex-1 sm:flex-none bg-blue-500 hover:bg-blue-600 text-white text-sm font-semibold py-2 px-3 rounded-lg flex items-center justify-center gap-2 transition-colors"
              >
                <IoAddCircleOutline className="h-4 w-4" />
                New
              </button>
              <button
                onClick={handleOpenProject}
                className="flex-1 sm:flex-none bg-emerald-500 hover:bg-emerald-600 text-white text-sm font-semibold py-2 px-3 rounded-lg flex items-center justify-center gap-2 transition-colors"
              >
                <PiUploadSimpleBold className="h-4 w-4" />
                Open
              </button>
            </div>
          </div>

          <div className="flex flex-col gap-3 lg:flex-row lg:items-center mb-4">
            <div className="relative flex-1">
              <FiSearch className="absolute left-3 top-1/2 transform -translate-y-1/2 text-gray-400" />
              <input
                type="text"
                placeholder="Search projects..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-10 pr-4 py-2 rounded-xl bg-slate-100 dark:bg-gray-800 border border-transparent focus:border-blue-400 text-slate-900 dark:text-white placeholder-slate-400 dark:placeholder-gray-500 transition-all"
              />
            </div>
            <div className="flex flex-wrap items-center gap-2 text-xs font-semibold text-slate-500 dark:text-gray-400">
              <span>Sort by:</span>
              <div className="flex gap-2">
                <button
                  onClick={() => handleSort("lastUpdate")}
                  className={`px-3 py-1.5 rounded-full border transition ${
                    sortBy === "lastUpdate"
                      ? "bg-blue-500 text-white border-blue-500"
                      : "bg-transparent text-slate-600 dark:text-gray-300 border-slate-200 dark:border-gray-700"
                  }`}
                >
                  Last Updated{" "}
                  {sortBy === "lastUpdate" && (sortAscending ? "↑" : "↓")}
                </button>
                <button
                  onClick={() => handleSort("fileName")}
                  className={`px-3 py-1.5 rounded-full border transition ${
                    sortBy === "fileName"
                      ? "bg-blue-500 text-white border-blue-500"
                      : "bg-transparent text-slate-600 dark:text-gray-300 border-slate-200 dark:border-gray-700"
                  }`}
                >
                  Name {sortBy === "fileName" && (sortAscending ? "↑" : "↓")}
                </button>
              </div>
            </div>
          </div>
          <div className="mt-1 flex-1 min-h-0">
            {isLoading ? (
              <div className="flex items-center justify-center h-full">
                <div className="animate-spin rounded-full h-10 w-10 border-b-2 border-blue-500"></div>
              </div>
            ) : filteredProjects.length === 0 ? (
              <div className="flex items-center justify-center h-full text-center text-slate-500 dark:text-gray-400 px-8">
                {searchQuery
                  ? "No projects found matching your search."
                  : "No projects in history. Create or open a project to get started!"}
              </div>
            ) : (
              <div className="space-y-3 h-full overflow-y-auto pr-1 custom-scrollbar">
                {filteredProjects.map((project) => (
                  <div
                    key={project.id}
                    className="bg-slate-50 dark:bg-gray-900 rounded-xl px-4 py-3 border border-slate-200 dark:border-gray-700 hover:border-blue-400 transition-colors"
                  >
                    <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3">
                      <div className="flex-1 min-w-0">
                        <h3 className="font-semibold text-base truncate">
                          {project.fileName}
                        </h3>
                        <p className="text-xs text-slate-500 dark:text-gray-400 truncate mt-1">
                          {project.filePath}
                        </p>
                        <p className="text-xs text-slate-400 dark:text-gray-500 mt-1">
                          Last updated: {formatDate(project.lastUpdate)}
                        </p>
                      </div>
                      <div className="flex items-center gap-2 lg:ml-4">
                        <button
                          onClick={() => handleLoadProject(project)}
                          className="bg-blue-500 hover:bg-blue-600 text-white px-4 py-2 rounded-lg flex items-center gap-2 text-sm transition-colors"
                          title="Open Project"
                        >
                          <PiUploadSimpleBold className="h-4 w-4" />
                          Open
                        </button>
                        <button
                          onClick={() =>
                            project.id && handleDeleteProject(project.id)
                          }
                          className="bg-red-500 hover:bg-red-600 text-white px-3 py-2 rounded-lg flex items-center gap-2 text-sm transition-colors"
                          title="Remove from History"
                        >
                          <IoTrashOutline className="h-4 w-4" />
                        </button>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});

export default DashboardPanel;
