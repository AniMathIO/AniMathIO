"use client";
import React from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import Modal from "react-modal";
import { IoIosSave } from "react-icons/io";
import { PiUploadSimpleBold } from "react-icons/pi";
import { addProjectToHistory } from "@/utils";


const ExportVideoPanel = observer(() => {
  const state = React.useContext(StateContext);
  const [resolution, setResolution] = React.useState('1920x1080');
  const [isRenderingModalOpen, setIsRenderingModalOpen] = React.useState(false);
  const [isSavingModalOpen, setIsSavingModalOpen] = React.useState(false);
  const [isLoadingModalOpen, setIsLoadingModalOpen] = React.useState(false);
  const [loadModalStatus, setLoadModalStatus] = React.useState<'loading' | 'success' | 'error'>('loading');
  const [loadErrorMessage, setLoadErrorMessage] = React.useState('');
  const [loadingProgress, setLoadingProgress] = React.useState(0); // Optional: for progress indication
  const [saveModalStatus, setSaveModalStatus] = React.useState<'saving' | 'success' | 'error'>('saving');
  const [saveErrorMessage, setSaveErrorMessage] = React.useState('');


  const resolutionOptions = [
    { value: '1920x1080', label: '1920x1080 (16:9 - Full HD)' },
    { value: '1280x720', label: '1280x720 (16:9 - HD)' },
    { value: '1080x1080', label: '1080x1080 (1:1 - Square)' },
    { value: '1080x1920', label: '1080x1920 (9:16 - Vertical Full HD)' },
    { value: '720x1280', label: '720x1280 (9:16 - Vertical HD)' },
    { value: '854x480', label: '854x480 (16:9 - SD)' },
    { value: '800x600', label: '800x600 (4:3 - SD)' },
    { value: '640x360', label: '640x360 (16:9 - SD)' },
  ];
  const handleResolutionChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const [width, height] = e.target.value.split('x').map(Number);
    state.setCanvasSize(width, height);
  };

  // Save to current file (if exists)
  const handleSave = async () => {
    if (!state.currentProjectFilePath) {
      // No current file, use Save As instead
      handleSaveAs();
      return;
    }

    try {
      // Show saving modal while serializing
      setSaveModalStatus('saving');
      setIsSavingModalOpen(true);

      // Serialize the project
      const buffer = await state.serialize();
      const fileData = Array.from(new Uint8Array(buffer));

      // Use Electron IPC to save to the current file path
      if (window.electron && window.electron.writeProjectFile && state.currentProjectFilePath) {
        try {
          // Save directly to the current file path (no dialog)
          const result = await window.electron.writeProjectFile(state.currentProjectFilePath, fileData);
          
          if (!result.success) {
            throw new Error(result.error || "Failed to write file");
          }

          // Update project history
          if (state.currentProjectFilePath && state.currentProjectFileName) {
            await addProjectToHistory(state.currentProjectFilePath, state.currentProjectFileName);
          }

          setSaveModalStatus('success');
          setIsSavingModalOpen(true);
          return;
        } catch (error) {
          console.error("Error saving to file path:", error);
          // If file doesn't exist or path is invalid, fall through to Save As
        }
      }

      // If we have a file handle (from File System Access API), use it
      if (state.currentProjectFileHandle && 'showSaveFilePicker' in window) {
        try {
          const blob = new Blob([buffer], { type: "application/octet-stream" });
          // @ts-ignore
          const writable = await state.currentProjectFileHandle.createWritable();
          await writable.write(blob);
          await writable.close();

          // Update project history
          if (state.currentProjectFilePath && state.currentProjectFileName) {
            await addProjectToHistory(state.currentProjectFilePath, state.currentProjectFileName);
          }

          setSaveModalStatus('success');
          setIsSavingModalOpen(true);
          return;
        } catch (error) {
          console.error("Error saving to file handle:", error);
          // Fall through to Save As
        }
      }

      // Fallback: use Save As
      handleSaveAs();
    } catch (error) {
      console.error("Error saving project:", error);
      setSaveModalStatus('error');
      setSaveErrorMessage(error instanceof Error ? error.message : 'Unknown error');
      setIsSavingModalOpen(true);
    }
  };

  // Save As - opens file picker
  const handleSaveAs = async () => {
    try {
      // Show saving modal while serializing
      setSaveModalStatus('saving');
      setIsSavingModalOpen(true);

      // Serialize the project
      const buffer = await state.serialize();
      const fileData = Array.from(new Uint8Array(buffer));
      const currentDate = new Date();
      const suggestedName = state.currentProjectFileName || `project-${currentDate.toISOString()}.animathio`;

      // Hide the saving modal before showing the native save dialog
      setIsSavingModalOpen(false);

      // Use Electron IPC to save file (gets full path)
      if (window.electron && window.electron.saveProjectFile) {
        try {
          const result = await window.electron.saveProjectFile(fileData, suggestedName);
          
          if (!result.success) {
            if (result.error === "File save cancelled") {
              setIsSavingModalOpen(false);
              return;
            }
            throw new Error(result.error || "Failed to save file");
          }

          // Store file information with full path
          if (result.fileName) {
            state.setCurrentProjectFileName(result.fileName);
          }
          if (result.filePath) {
            state.setCurrentProjectFilePath(result.filePath);
          }
          state.setCurrentProjectFileHandle(null); // Can't store file handle through IPC

          // Add to project history with full path
          if (result.filePath && result.fileName) {
            await addProjectToHistory(result.filePath, result.fileName);
          }

          // Show success modal
          setSaveModalStatus('success');
          setIsSavingModalOpen(true);
          return;
        } catch (error) {
          console.error("Error saving project via IPC:", error);
          setSaveModalStatus('error');
          setSaveErrorMessage(error instanceof Error ? error.message : 'Unknown error');
          setIsSavingModalOpen(true);
          return;
        }
      }

      // Fallback: Try File System Access API if available
      if ('showSaveFilePicker' in window) {
        try {
          // @ts-ignore
          const fileHandle = await window.showSaveFilePicker({
            suggestedName: suggestedName,
            types: [{
              description: 'Animathio Project File',
              accept: { 'application/octet-stream': ['.animathio'] }
            }]
          });

          const blob = new Blob([buffer], { type: "application/octet-stream" });
          // @ts-ignore
          const writable = await fileHandle.createWritable();
          await writable.write(blob);
          await writable.close();

          // Store file handle (but we can't get full path from FileSystemFileHandle)
          state.setCurrentProjectFileHandle(fileHandle);
          state.setCurrentProjectFileName(fileHandle.name);
          // Note: We can't get full path from FileSystemFileHandle, so we store the name
          // This is a limitation of the File System Access API
          state.setCurrentProjectFilePath(fileHandle.name);

          // Add to project history (with filename only, as we can't get full path)
          await addProjectToHistory(fileHandle.name, fileHandle.name);

          // Show success modal
          setSaveModalStatus('success');
          setIsSavingModalOpen(true);
          return;
        } catch (fsError: any) {
          // User might have canceled
          if (fsError.name === 'AbortError') {
            setIsSavingModalOpen(false);
            return;
          }
          console.log("File System Access API failed, falling back:", fsError);
        }
      }

      // Fallback to traditional download method
      const blob = new Blob([buffer], { type: "application/octet-stream" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = suggestedName;
      link.click();

      setTimeout(() => {
        URL.revokeObjectURL(url);
        setSaveModalStatus('success');
        setIsSavingModalOpen(true);
      }, 1000);

    } catch (error) {
      console.error("Error saving project:", error);
      setSaveModalStatus('error');
      setSaveErrorMessage(error instanceof Error ? error.message : 'Unknown error');
      setIsSavingModalOpen(true);
    }
  };
  const handleLoadProject = async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".animathio";

    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) {
        // Reset and show loading modal
        setLoadModalStatus('loading');
        setLoadingProgress(0);
        setIsLoadingModalOpen(true);

        const reader = new FileReader();
        const startTime = Date.now();

        // Setup progress monitoring
        reader.onprogress = (event) => {
          if (event.lengthComputable) {
            setLoadingProgress(Math.round((event.loaded / event.total) * 100));
          }
        };

        reader.onload = async (e) => {
          try {
            const buffer = new Uint8Array(e.target?.result as ArrayBuffer);

            // Deserialize the project
            await state.deserialize(buffer.buffer);

            // Store file information
            state.setCurrentProjectFileName(file.name);
            state.setCurrentProjectFilePath(file.name);
            state.setCurrentProjectFileHandle(null); // Can't store file handle from file input

            // Add to project history
            await addProjectToHistory(file.name, file.name);

            // Calculate how long the operation took
            const operationTime = Date.now() - startTime;
            const remainingTime = Math.max(0, 3000 - operationTime);

            // Wait for the minimum display time before showing success
            setTimeout(() => {
              // Show success state - without auto-closing
              setLoadModalStatus('success');
              setLoadingProgress(100); // Ensure progress shows complete
            }, remainingTime);

          } catch (error) {
            console.error("Error loading project:", error);

            // Set error state
            setLoadModalStatus('error');
            setLoadErrorMessage(error instanceof Error ? error.message : 'Failed to load project. The file might be corrupted or incompatible.');
          }
        };

        reader.onerror = () => {
          console.error("File reading error");
          setLoadModalStatus('error');
          setLoadErrorMessage('Failed to read the file. Please try again.');
        };

        reader.readAsArrayBuffer(file);
      }
    };

    input.click();
  };

  return (
    <>
      <div className="text-lg px-[16px] pt-[16px] pb-[15px] font-semibold text-black dark:text-white">
        Export
      </div>
      {/* Set max time from number input */}
      <div className="px-[16px]">
        <div className="flex flex-row items-center my-2">
          <div className="text-base font-semibold mr-2 text-black dark:text-white">Video Length:</div>
          <label htmlFor="video-length" className="sr-only">Video Length</label>
          <input
            id="video-length"
            type="number"
            className="rounded text-center text-black dark:text-white bg-white dark:bg-gray-800 border-slate-200 dark:border-gray-600 placeholder-slate-400 dark:placeholder-gray-400 contrast-more:border-slate-400 contrast-more:placeholder-slate-500 max-w-[50px] mr-2"
            value={state.maxTime / 1000}
            onChange={(e) => {
              const value = e.target.value;
              state.setMaxTime(Number(value) * 1000);
            }}
            placeholder="Length in seconds"
          />
          <div className="text-black dark:text-white">secs</div>
        </div>
        <div className="flex flex-row items-center my-2">
          <div className="text-base font-semibold mr-2 text-black dark:text-white">Canvas Resolution:</div>
          <label htmlFor="canvas-resolution" className="sr-only">Canvas Resolution</label>
          <select
            id="canvas-resolution"
            name="canvas-resolution"
            value={`${state.canvas_width}x${state.canvas_height}`}
            onChange={handleResolutionChange}
            className="rounded-md text-base text-black dark:text-white bg-white dark:bg-gray-800 w-24 border-slate-200 dark:border-gray-600 placeholder-slate-400 dark:placeholder-gray-400 contrast-more:border-slate-400 contrast-more:placeholder-slate-500"
          >
            {resolutionOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </div>
      </div >
      {/*  Format selection with radio button */}
      <div className="px-[16px]" >
        <div className="text-base font-semibold mr-2 text-black dark:text-white">Video Format:</div>
        <div className="flex flex-row items-center my-2">
          <label htmlFor="video-format" className="sr-only">Video Format</label>
          <input
            id="video-format"
            type="radio"
            className="mr-2"
            name="video-format"
            value="mp4"
            checked={state.selectedVideoFormat === "mp4"}
            onChange={(e) => {
              state.setVideoFormat("mp4");
            }}
            placeholder="Video Format"
          />
          <div className="text-md mr-2 text-black dark:text-white">MP4</div>
          <label htmlFor="video-format" ></label>
          <input
            id="video-format"
            type="radio"
            className="mr-2"
            name="video-format"
            value="webm"
            checked={state.selectedVideoFormat === "webm"}
            onChange={(e) => {
              state.setVideoFormat("webm");
            }}
            placeholder="Video Format"
          />
          <div className="text-md mr-2 text-black dark:text-white">webm</div>
        </div>
      </div>

      <button
        className="bg-gray-500 hover:bg-gray-900 text-white font-bold py-2 px-2 rounded-lg m-4"
        onClick={() => {
          state.handleSeek(0);
          state.setSelectedElement(null);
          setTimeout(() => {
            state.setPlaying(true);
            state.saveCanvasToVideoWithAudio();
          }, 1000);
          setIsRenderingModalOpen(true);
          setTimeout(() => {
            setIsRenderingModalOpen(false);
            // If the rendering option is mp4 we need to wait additional 15 seconds to do the conversion
          }, state.maxTime + 1000 + (state.selectedVideoFormat === "mp4" ? 15000 : 0));
        }}
      >
        Export Video ({state.maxTime / 1000} seconds)
      </button >

      <div className="text-base font-semibold m-4 mb-0 text-black dark:text-white">Save / Save As Project:</div>
      {/* Save button - saves to current file */}
      <button
        className="bg-gray-500 hover:bg-gray-900 text-white font-bold py-2 px-2 rounded-lg ml-4 mr-2 mb-2"
        onClick={handleSave}
        title={state.currentProjectFilePath ? `Save to ${state.currentProjectFileName}` : "Save (will use Save As)"}
      >
        <div className="flex justify-center align-middle gap-1">
          <IoIosSave className="h-6 w-6" />
          Save
        </div>
      </button>

      {/* Save As button - opens file picker */}
      <button
        className="bg-gray-500 hover:bg-gray-900 text-white font-bold py-2 px-2 rounded-lg mr-4 mb-2"
        onClick={handleSaveAs}
      >
        <div className="flex justify-center align-middle gap-1">
          <IoIosSave className="h-6 w-6" />
          Save As
        </div>
      </button>


      {/* Video Rendering Modal */}
      <Modal
        isOpen={isRenderingModalOpen}
        onRequestClose={() => setIsRenderingModalOpen(false)}
        contentLabel="Video Rendering Modal"
        className="fixed inset-0 flex items-center justify-center z-50 bg-black/50"
        overlayClassName="fixed inset-0 z-40 bg-black/60"
        shouldCloseOnOverlayClick={false}
      >
        <div className="bg-white dark:bg-gray-800 p-6 rounded-sm shadow-lg max-w-md mx-auto">
          <div className="flex items-center mb-4">
            <div className="mr-4">
              <svg
                className="animate-spin h-8 w-8 text-blue-500"
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
              >
                <circle
                  className="opacity-25"
                  cx="12"
                  cy="12"
                  r="10"
                  stroke="currentColor"
                  strokeWidth="4"
                ></circle>
                <path
                  className="opacity-75"
                  fill="currentColor"
                  d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                ></path>
              </svg>
            </div>
            <div>
              <h2 className="text-xl font-bold text-black dark:text-white">Video Rendering</h2>
              <p className="text-gray-700 dark:text-gray-300">Please wait while the video is being rendered...</p>
              <br />
              <p className="text-gray-800 dark:text-gray-200"><b>Note:</b> rendering in MP4 results in higher quality, but takes longer.</p>
            </div>
          </div>
        </div>
      </Modal>

      {/* Saving Project Modal */}
      <Modal
        isOpen={isSavingModalOpen}
        onRequestClose={() => {
          // Only allow closing if we're not in the 'saving' state
          if (saveModalStatus !== 'saving') {
            setIsSavingModalOpen(false);
            // Reset status for next time
            setSaveModalStatus('saving');
          }
        }}
        contentLabel="Project Saving Modal"
        className="fixed inset-0 flex items-center justify-center z-50 bg-black/50"
        overlayClassName="fixed inset-0 z-40 bg-black/60"
        shouldCloseOnOverlayClick={saveModalStatus !== 'saving'}
      >
        <div className="bg-white dark:bg-gray-800 p-6 rounded-sm shadow-lg max-w-md mx-auto">
          {saveModalStatus === 'saving' && (
            <div className="flex items-center mb-4">
              <div className="mr-4">
                <svg
                  className="animate-spin h-8 w-8 text-green-500"
                  xmlns="http://www.w3.org/2000/svg"
                  fill="none"
                  viewBox="0 0 24 24"
                >
                  <circle
                    className="opacity-25"
                    cx="12"
                    cy="12"
                    r="10"
                    stroke="currentColor"
                    strokeWidth="4"
                  ></circle>
                  <path
                    className="opacity-75"
                    fill="currentColor"
                    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                  ></path>
                </svg>
              </div>
              <div>
                <h2 className="text-xl font-bold text-black dark:text-white">Saving Project</h2>
                <p className="text-gray-700 dark:text-gray-300">Please wait while your project is being saved...</p>
                <br />
                <p className="text-gray-800 dark:text-gray-200">This may take a moment if your project contains large media files.</p>
              </div>
            </div>
          )}

          {saveModalStatus === 'success' && (
            <div className="flex items-center mb-4">
              <div className="mr-4">
                <svg
                  className="h-8 w-8 text-green-500"
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path>
                  <polyline points="22 4 12 14.01 9 11.01"></polyline>
                </svg>
              </div>
              <div>
                <h2 className="text-xl font-bold text-green-600 dark:text-green-400">Project Saved Successfully!</h2>
                <p className="text-gray-700 dark:text-gray-300">Your project has been saved.</p>
                <button
                  className="mt-4 bg-green-500 hover:bg-green-700 text-white font-bold py-2 px-4 rounded-sm"
                  onClick={() => {
                    setIsSavingModalOpen(false);
                    setSaveModalStatus('saving');
                  }}
                >
                  Close
                </button>
              </div>
            </div>
          )}

          {saveModalStatus === 'error' && (
            <div className="flex items-center mb-4">
              <div className="mr-4">
                <svg
                  className="h-8 w-8 text-red-500"
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <circle cx="12" cy="12" r="10"></circle>
                  <line x1="15" y1="9" x2="9" y2="15"></line>
                  <line x1="9" y1="9" x2="15" y2="15"></line>
                </svg>
              </div>
              <div>
                <h2 className="text-xl font-bold text-red-600 dark:text-red-400">Error Saving Project</h2>
                <p className="text-gray-700 dark:text-gray-300">An error occurred while saving your project:</p>
                <p className="text-red-600 dark:text-red-400 font-medium mt-2 p-2 bg-red-100 dark:bg-red-900 rounded-sm">{saveErrorMessage || 'Unknown error occurred'}</p>
                <button
                  className="mt-4 bg-red-500 hover:bg-red-700 text-white font-bold py-2 px-4 rounded-sm"
                  onClick={() => {
                    setIsSavingModalOpen(false);
                    setSaveModalStatus('saving');
                  }}
                >
                  Close
                </button>
              </div>
            </div>
          )}
        </div>
      </Modal>

      {/* Loading Project Modal */}
      <Modal
        isOpen={isLoadingModalOpen}
        onRequestClose={() => {
          // Only allow closing if we're not in the 'loading' state
          if (loadModalStatus !== 'loading') {
            setIsLoadingModalOpen(false);
            setLoadModalStatus('loading');
          }
        }}
        contentLabel="Project Loading Modal"
        className="fixed inset-0 flex items-center justify-center z-50 bg-black/50"
        overlayClassName="fixed inset-0 z-40 bg-black/60"
        shouldCloseOnOverlayClick={loadModalStatus !== 'loading'}
      >
        <div className="bg-white dark:bg-gray-800 p-6 rounded-sm shadow-lg max-w-md mx-auto">
          {loadModalStatus === 'loading' && (
            <div className="flex items-center mb-4">
              <div className="mr-4">
                <svg
                  className="animate-spin h-8 w-8 text-purple-500"
                  xmlns="http://www.w3.org/2000/svg"
                  fill="none"
                  viewBox="0 0 24 24"
                >
                  <circle
                    className="opacity-25"
                    cx="12"
                    cy="12"
                    r="10"
                    stroke="currentColor"
                    strokeWidth="4"
                  ></circle>
                  <path
                    className="opacity-75"
                    fill="currentColor"
                    d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
                  ></path>
                </svg>
              </div>
              <div>
                <h2 className="text-xl font-bold text-black dark:text-white">Loading Project</h2>
                <p className="text-gray-700 dark:text-gray-300">Please wait while your project is being loaded...</p>

                {/* Progress bar */}
                {loadingProgress > 0 && (
                  <div className="mt-3">
                    <div className="w-full bg-gray-200 rounded-full h-2.5">
                      <div
                        className="bg-purple-600 h-2.5 rounded-full"
                        style={{ width: `${loadingProgress}%` }}
                      ></div>
                    </div>
                    <p className="text-xs text-gray-500 dark:text-gray-400 text-right mt-1">{loadingProgress}%</p>
                  </div>
                )}
              </div>
            </div>
          )}

          {loadModalStatus === 'success' && (
            <div className="flex items-center mb-4">
              <div className="mr-4">
                <svg
                  className="h-8 w-8 text-purple-500"
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"></path>
                  <polyline points="22 4 12 14.01 9 11.01"></polyline>
                </svg>
              </div>
              <div>
                <h2 className="text-xl font-bold text-purple-600 dark:text-purple-400">Project Loaded Successfully!</h2>
                <p className="text-gray-700 dark:text-gray-300">Your project is now ready to use.</p>
                <button
                  className="mt-4 bg-purple-500 hover:bg-purple-700 text-white font-bold py-2 px-4 rounded-sm"
                  onClick={() => {
                    setIsLoadingModalOpen(false);
                    setLoadModalStatus('loading');
                  }}
                >
                  Close
                </button>
              </div>
            </div>
          )}

          {loadModalStatus === 'error' && (
            <div className="flex items-center mb-4">
              <div className="mr-4">
                <svg
                  className="h-8 w-8 text-red-500"
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <circle cx="12" cy="12" r="10"></circle>
                  <line x1="15" y1="9" x2="9" y2="15"></line>
                  <line x1="9" y1="9" x2="15" y2="15"></line>
                </svg>
              </div>
              <div>
                <h2 className="text-xl font-bold text-red-600 dark:text-red-400">Error Loading Project</h2>
                <p className="text-gray-700 dark:text-gray-300">An error occurred while loading your project:</p>
                <p className="text-red-600 dark:text-red-400 font-medium mt-2 p-2 bg-red-100 dark:bg-red-900 rounded-sm">{loadErrorMessage}</p>
                <button
                  className="mt-4 bg-red-500 hover:bg-red-700 text-white font-bold py-2 px-4 rounded-sm"
                  onClick={() => {
                    setIsLoadingModalOpen(false);
                    setLoadModalStatus('loading');
                  }}
                >
                  Close
                </button>
              </div>
            </div>
          )}
        </div>
      </Modal>
    </>
  );
});

export default ExportVideoPanel
