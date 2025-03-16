"use client";
import React, { useState, useRef, useEffect, useMemo, useCallback } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { MicrophoneIcon, StopIcon, TrashIcon, CheckIcon, AdjustmentsHorizontalIcon } from "@heroicons/react/24/solid";
import WaveSurfer from "wavesurfer.js";
import RecordPlugin from 'wavesurfer.js/dist/plugins/record';

const AudioRecorder = observer(() => {
    const state = React.useContext(StateContext);
    const [isRecording, setIsRecording] = useState(false);
    const [isPaused, setIsPaused] = useState(false);
    const [recordingTime, setRecordingTime] = useState(0);
    const [audioUrl, setAudioUrl] = useState<string | null>(null);
    const [visualizerData, setVisualizerData] = useState<number[]>([]);
    const [permissionError, setPermissionError] = useState<string | null>(null);
    const [audioDevices, setAudioDevices] = useState<MediaDeviceInfo[]>([]);
    const [selectedDeviceId, setSelectedDeviceId] = useState<string>("");
    const [showDeviceSelector, setShowDeviceSelector] = useState(false);
    const [visualizerReady, setVisualizerReady] = useState(false);

    // Element ID for the visualizer - must be unique
    const visualizerContainerId = "audio-recorder-visualizer";

    const mediaRecorderRef = useRef<MediaRecorder | null>(null);
    const audioChunksRef = useRef<Blob[]>([]);
    const streamRef = useRef<MediaStream | null>(null);
    const wavesurferRef = useRef<WaveSurfer | null>(null);
    const recordPluginRef = useRef<any | null>(null);
    const timeInterval = useRef<NodeJS.Timeout | null>(null);
    const visualizerInitializedRef = useRef<boolean>(false);

    // Fetch available audio input devices
    const getAudioDevices = async () => {
        try {
            // First ensure we have permissions to enumerate devices
            await navigator.mediaDevices.getUserMedia({ audio: true })
                .then(stream => {
                    // Stop the stream immediately, we just needed permission
                    stream.getTracks().forEach(track => track.stop());
                });

            const devices = await navigator.mediaDevices.enumerateDevices();
            const audioInputs = devices.filter(device => device.kind === 'audioinput');

            console.info("Available audio devices:", audioInputs);
            setAudioDevices(audioInputs);

            // Set default device if we have devices and none is selected
            if (audioInputs.length > 0 && !selectedDeviceId) {
                // Try to find default device
                const defaultDevice = audioInputs.find(device => device.deviceId === 'default' || device.label.toLowerCase().includes('default'));
                setSelectedDeviceId(defaultDevice ? defaultDevice.deviceId : audioInputs[0].deviceId);
            }
        } catch (error) {
            console.error("Error getting audio devices:", error);
            setPermissionError("Could not access audio devices. Please check permissions.");
        }
    };

    // Cleanup function for WaveSurfer
    const cleanupWaveSurfer = useCallback(() => {
        if (recordPluginRef.current) {
            try {
                if (recordPluginRef.current.isRecording && recordPluginRef.current.isRecording()) {
                    recordPluginRef.current.stopRecording();
                }
            } catch (err) {
                console.error("Error stopping wavesurfer recording:", err);
            }
            recordPluginRef.current = null;
        }

        if (wavesurferRef.current) {
            try {
                wavesurferRef.current.destroy();
            } catch (err) {
                console.error("Error destroying wavesurfer:", err);
            }
            wavesurferRef.current = null;
        }

        if (timeInterval.current) {
            clearInterval(timeInterval.current);
            timeInterval.current = null;
        }

        visualizerInitializedRef.current = false;
        setVisualizerReady(false);
    }, []);

    // Initialize WaveSurfer - separated from UI render cycle
    const initializeVisualizer = useCallback(async () => {
        // If already initialized or recording is in progress, skip
        if (visualizerInitializedRef.current || isRecording) return;

        // First clean up any existing instances
        cleanupWaveSurfer();

        // Wait for DOM to be ready
        await new Promise(resolve => setTimeout(resolve, 100));

        // Get the container element
        const container = document.getElementById(visualizerContainerId);
        if (!container) {
            console.warn(`Visualizer container #${visualizerContainerId} not found`);
            return;
        }

        try {
            console.info("Creating WaveSurfer instance");

            // Create WaveSurfer instance
            const wavesurfer = WaveSurfer.create({
                container: `#${visualizerContainerId}`,
                waveColor: '#00a0f5', 
                progressColor: '#027bbca9',
                height: 80,
                cursorWidth: 0,
                interact: false,
                fillParent: true,
                minPxPerSec: 50,
                normalize: true,
                autoCenter: true,
                barWidth: 2,
                barGap: 1,
                barRadius: 2,
            });

            wavesurferRef.current = wavesurfer;

            // Create and register Record plugin
            const recordPlugin = wavesurfer.registerPlugin(
                RecordPlugin.create({
                    renderRecordedAudio: false,
                    scrollingWaveform: false,
                    continuousWaveform: true,
                    continuousWaveformDuration: 30,
                })
            );

            recordPluginRef.current = recordPlugin;

            // Add event listeners
            // if (recordPlugin.on) {
            //     recordPlugin.on('record-progress', (time: number) => {
            //         console.info("WaveSurfer recording progress:", Math.floor(time / 1000));
            //     });
            // }

            visualizerInitializedRef.current = true;
            setVisualizerReady(true);
            console.info("WaveSurfer initialized successfully");
        } catch (error) {
            console.error("Error initializing WaveSurfer:", error);
            visualizerInitializedRef.current = false;
            setVisualizerReady(false);
        }
    }, [cleanupWaveSurfer, isRecording, visualizerContainerId]);

    // Listen for device changes
    useEffect(() => {
        getAudioDevices();

        // Set up a listener for device changes
        navigator.mediaDevices.addEventListener('devicechange', getAudioDevices);

        return () => {
            navigator.mediaDevices.removeEventListener('devicechange', getAudioDevices);
        };
    }, []);

    // Clean up on unmount
    useEffect(() => {
        return cleanupWaveSurfer;
    }, [cleanupWaveSurfer]);

    // Recording timer effect
    useEffect(() => {
        if (timeInterval.current) {
            clearInterval(timeInterval.current);
            timeInterval.current = null;
        }

        if (isRecording && !isPaused) {
            timeInterval.current = setInterval(() => {
                setRecordingTime(prev => prev + 1);
            }, 1000);
        }

        return () => {
            if (timeInterval.current) {
                clearInterval(timeInterval.current);
                timeInterval.current = null;
            }
        };
    }, [isRecording, isPaused]);

    // Check if we're running in Electron
    const isElectron = () => {
        // @ts-ignore
        return window?.electron !== undefined;
    };

    const requestElectronPermission = async () => {
        if (!isElectron()) return true;

        try {
            // Use the exposed function from preload
            // @ts-ignore
            if (window.electron && window.electron.requestMicrophonePermission) {
                // @ts-ignore
                const permission = await window.electron.requestMicrophonePermission();
                return permission === 'granted';
            }

            // Fallback if the API isn't available
            return true;
        } catch (error) {
            console.error("Error requesting Electron permission:", error);
            return false;
        }
    };

    const startRecording = async () => {
        setPermissionError(null);
        setRecordingTime(0);

        try {
            // First check if we need to request Electron-specific permissions
            const permissionGranted = await requestElectronPermission();
            if (!permissionGranted) {
                setPermissionError("Microphone access denied. Please enable microphone access in your system settings.");
                return;
            }

            // Prepare the UI for recording - this will make the container visible
            setIsRecording(true);

            // Wait for render cycle to complete
            await new Promise(resolve => setTimeout(resolve, 100));

            // Initialize the visualizer now that the container is in the DOM
            await initializeVisualizer();

            // Configure audio constraints based on selected device
            const audioConstraints: MediaTrackConstraints = {
                echoCancellation: true,
                noiseSuppression: true,
                autoGainControl: true
            };

            // Only set deviceId if we have a valid selection
            if (selectedDeviceId) {
                audioConstraints.deviceId = { exact: selectedDeviceId };
            }

            console.info("Starting recording with device:", selectedDeviceId);

            // Get media stream
            const stream = await navigator.mediaDevices.getUserMedia({
                audio: audioConstraints
            });

            console.info("Media stream obtained successfully");
            streamRef.current = stream;

            // Check if we actually got audio tracks
            if (stream.getAudioTracks().length === 0) {
                throw new Error("No audio tracks found in the media stream");
            }

            // Start WaveSurfer recording if available
            if (recordPluginRef.current && visualizerInitializedRef.current) {
                try {
                    await recordPluginRef.current.startRecording({
                        deviceId: selectedDeviceId || undefined
                    });
                    console.info("WaveSurfer recording started successfully");
                } catch (err) {
                    console.warn("Could not start WaveSurfer recording:", err);
                }
            } else {
                console.warn("Record plugin not initialized, continuing without visualization");
            }

            // Check for supported MIME types
            const mimeTypes = [
                'audio/webm',
                'audio/webm;codecs=opus',
                'audio/mp4',
                'audio/ogg',
                'audio/ogg;codecs=opus'
            ];

            let selectedMimeType = '';
            for (const type of mimeTypes) {
                if (MediaRecorder.isTypeSupported(type)) {
                    selectedMimeType = type;
                    console.info("Using MIME type:", selectedMimeType);
                    break;
                }
            }

            if (!selectedMimeType) {
                console.warn("No supported MIME types found, falling back to default");
            }

            // Create MediaRecorder with explicit MIME type if supported
            const options: MediaRecorderOptions = {};
            if (selectedMimeType) {
                options.mimeType = selectedMimeType;
            }

            const mediaRecorder = new MediaRecorder(stream, options);
            mediaRecorderRef.current = mediaRecorder;

            mediaRecorder.ondataavailable = (event) => {
                if (event.data.size > 0) {
                    audioChunksRef.current.push(event.data);
                }
            };

            mediaRecorder.onstop = () => {
                console.info("MediaRecorder stopped, chunks:", audioChunksRef.current.length);
                if (audioChunksRef.current.length === 0) {
                    setPermissionError("No audio data was recorded. Please check your microphone settings.");
                    return;
                }

                const mimeType = mediaRecorder.mimeType || 'audio/webm';
                console.info("Creating blob with MIME type:", mimeType);
                const audioBlob = new Blob(audioChunksRef.current, { type: mimeType });
                console.info("Blob created, size:", audioBlob.size);

                const url = URL.createObjectURL(audioBlob);
                setAudioUrl(url);

                // Clean up stream tracks
                if (streamRef.current) {
                    streamRef.current.getTracks().forEach(track => {
                        track.stop();
                    });
                }
            };

            audioChunksRef.current = [];

            // Request data at regular intervals for more reliable recording
            console.info("Starting MediaRecorder");
            mediaRecorder.start(100);

        } catch (error) {
            console.error("Error accessing microphone:", error);

            // Stop WaveSurfer recording if there was an error
            if (recordPluginRef.current && visualizerInitializedRef.current) {
                try {
                    if (recordPluginRef.current.isRecording && recordPluginRef.current.isRecording()) {
                        recordPluginRef.current.stopRecording();
                    }
                } catch (e) {
                    console.warn("Error stopping WaveSurfer recording:", e);
                }
            }

            setIsRecording(false);

            // Provide specific error messages based on the error
            if (error instanceof DOMException) {
                if (error.name === 'NotAllowedError' || error.name === 'PermissionDeniedError') {
                    setPermissionError("Microphone access denied. Please enable microphone access in your system settings.");
                } else if (error.name === 'NotFoundError' || error.name === 'DevicesNotFoundError') {
                    setPermissionError("No microphone found. Please connect a microphone and try again.");
                } else if (error.name === 'NotReadableError' || error.name === 'AbortError') {
                    setPermissionError("Could not access the selected microphone. It may be in use by another application.");
                } else if (error.name === 'OverconstrainedError') {
                    setPermissionError("The selected microphone is no longer available. Please select another device.");
                    // Refresh device list
                    getAudioDevices();
                } else {
                    setPermissionError(`Microphone error: ${error.message}`);
                }
            } else if (error instanceof Error) {
                setPermissionError(`Error: ${error.message}`);
            } else {
                setPermissionError("An error occurred while accessing the microphone.");
            }
        }
    };

    const pauseRecording = () => {
        if (mediaRecorderRef.current && isRecording) {
            if (!isPaused) {
                // Pause WaveSurfer recording
                if (recordPluginRef.current && visualizerInitializedRef.current) {
                    try {
                        if (recordPluginRef.current.isRecording && recordPluginRef.current.isRecording()) {
                            recordPluginRef.current.pauseRecording();
                            console.info("WaveSurfer recording paused");
                        }
                    } catch (e) {
                        console.warn("Error pausing WaveSurfer recording:", e);
                    }
                }

                // Some versions of MediaRecorder don't support pause
                try {
                    mediaRecorderRef.current.pause();
                    console.info("MediaRecorder paused");
                } catch (e) {
                    console.warn("MediaRecorder pause not supported", e);
                    // If pause is not supported, we'll just stop the recording
                    stopRecording();
                    return;
                }

                setIsPaused(true);
            } else {
                // Resume WaveSurfer recording
                if (recordPluginRef.current && visualizerInitializedRef.current) {
                    try {
                        if (recordPluginRef.current.isPaused && recordPluginRef.current.isPaused()) {
                            recordPluginRef.current.resumeRecording();
                            console.info("WaveSurfer recording resumed");
                        }
                    } catch (e) {
                        console.warn("Error resuming WaveSurfer recording:", e);
                    }
                }

                try {
                    mediaRecorderRef.current.resume();
                    console.info("MediaRecorder resumed");
                } catch (e) {
                    console.warn("MediaRecorder resume not supported", e);
                    // If resume is not supported, we might need to restart recording
                }

                setIsPaused(false);
            }
        }
    };

    const stopRecording = () => {
        if (mediaRecorderRef.current && isRecording) {
            try {
                console.info("Stopping MediaRecorder");
                mediaRecorderRef.current.stop();

                // Stop WaveSurfer recording
                if (recordPluginRef.current && visualizerInitializedRef.current) {
                    try {
                        if (recordPluginRef.current.isRecording &&
                            (recordPluginRef.current.isRecording() ||
                                (recordPluginRef.current.isPaused && recordPluginRef.current.isPaused()))) {
                            recordPluginRef.current.stopRecording();
                            console.info("WaveSurfer recording stopped");
                        }
                    } catch (e) {
                        console.warn("Error stopping WaveSurfer recording:", e);
                    }
                }

            } catch (e) {
                console.error("Error stopping recorder:", e);
                // Even if there's an error, we should update the UI
            }

            setIsRecording(false);
            setIsPaused(false);
        }
    };

    const discardRecording = () => {
        if (audioUrl) {
            console.info("Discarding recording");
            URL.revokeObjectURL(audioUrl);
            setAudioUrl(null);
        }

        setRecordingTime(0);
        audioChunksRef.current = [];

        // Cleanup WaveSurfer
        cleanupWaveSurfer();
    };

    const saveRecording = () => {
        if (audioUrl) {
            console.info("Saving recording");

            // Add to the audio resources
            state.addAudioResource(audioUrl);

            // Find the index of the newly added audio
            const index = state.audios.length - 1;
            console.info("Audio added at index:", index);

            // Create a new audio element to preload the audio
            const preloadAudio = new Audio(audioUrl);
            preloadAudio.addEventListener('loadedmetadata', () => {
                console.info("Audio metadata loaded, duration:", preloadAudio.duration);

                // Now we know the audio is loaded, add it to the timeline
                state.addAudio(index);

                // Reset after saving
                setAudioUrl(null);
                setRecordingTime(0);

                // Cleanup WaveSurfer
                cleanupWaveSurfer();
            });

            // Start loading the audio
            preloadAudio.load();
        }
    };

    // Format time as mm:ss
    const formatTime = (seconds: number) => {
        const mins = Math.floor(seconds / 60);
        const secs = seconds % 60;
        return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
    };

    // Helper function to display device names more nicely
    const formatDeviceName = (device: MediaDeviceInfo) => {
        if (!device.label) return "Unnamed Microphone";

        // Handle common built-in device naming patterns
        let label = device.label;

        // Remove common prefixes like "Default - "
        label = label.replace(/^Default - /i, '');

        // Truncate very long names
        if (label.length > 40) {
            label = label.substring(0, 37) + '...';
        }

        return label;
    };

    // Toggle the microphone selector
    const toggleDeviceSelector = () => {
        if (!showDeviceSelector) {
            // Refresh the device list when opening
            getAudioDevices();
        }
        setShowDeviceSelector(!showDeviceSelector);
    };

    return (
        <div className="p-4 bg-white dark:bg-gray-800 rounded-lg mb-4">
            <div className="flex justify-between items-center mb-2">
                <h3 className="text-sm font-semibold">Voice Narration</h3>

                {!isRecording && !audioUrl && audioDevices.length > 0 && (
                    <button
                        onClick={toggleDeviceSelector}
                        className="text-gray-600 hover:text-black dark:text-gray-400 dark:hover:text-white transition-colors"
                        title="Select microphone"
                    >
                        <AdjustmentsHorizontalIcon className="w-5 h-5" />
                    </button>
                )}
            </div>

            {showDeviceSelector && !isRecording && !audioUrl && (
                <div className="mb-3 p-2 bg-slate-200 dark:bg-gray-700 rounded-lg">
                    <label className="block text-xs text-black dark:text-gray-400 mb-1">Select Microphone</label>
                    <select
                        aria-label="Select microphone"
                        value={selectedDeviceId}
                        onChange={(e) => setSelectedDeviceId(e.target.value)}
                        className="w-full text-black bg-white dark:bg-gray-900 dark:text-white rounded py-1.5 px-2 text-sm"
                    >
                        {audioDevices.map((device) => (
                            <option key={device.deviceId} value={device.deviceId}>
                                {formatDeviceName(device)}
                            </option>
                        ))}
                    </select>
                    <button
                        onClick={getAudioDevices}
                        className="mt-2 text-xs text-blue-700 hover:text-blue-900 hover:font-medium darK:text-blue-400 dark:hover:text-blue-300"
                    >
                        Refresh device list
                    </button>
                </div>
            )}

            {permissionError && (
                <div className="mb-3 p-2 bg-red-800 text-white rounded text-sm">
                    {permissionError}
                </div>
            )}

            {!isRecording && !audioUrl && (
                <button
                    onClick={startRecording}
                    className="w-full py-2 px-4 flex items-center justify-center bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors"
                >
                    <MicrophoneIcon className="w-5 h-5 mr-2" />
                    Record Audio
                </button>
            )}

            {isRecording && (
                <div className="space-y-3">
                    <div className="flex justify-between items-center">
                        <span className="text-red-500 font-mono">{formatTime(recordingTime)}</span>
                        <span className={`rounded-full w-3 h-3 ${isPaused ? 'bg-yellow-500' : 'bg-red-500 animate-pulse'}`}></span>
                    </div>

                    {/* Using fixed ID for the visualizer container */}
                    <div
                        id={visualizerContainerId}
                        className="w-full h-20 dark:bg-gray-700 border-[1px] dark:border-0 border-gray-300 rounded-md overflow-hidden"
                    ></div>

                    <div className="flex justify-between space-x-2">
                        <button
                            onClick={pauseRecording}
                            className={`flex-1 py-1.5 px-3 flex items-center justify-center ${isPaused ? 'bg-green-600 hover:bg-green-700' : 'bg-yellow-600 hover:bg-yellow-700'} text-white rounded-lg transition-colors`}
                        >
                            {isPaused ? 'Resume' : 'Pause'}
                        </button>
                        <button
                            onClick={stopRecording}
                            className="flex-1 py-1.5 px-3 flex items-center justify-center bg-red-600 hover:bg-red-700 text-white rounded-lg transition-colors"
                        >
                            <StopIcon className="w-4 h-4 mr-1" />
                            Stop
                        </button>
                    </div>
                </div>
            )}

            {audioUrl && !isRecording && (
                <div className="space-y-3">
                    <div className="rounded-lg overflow-hidden">
                        <audio
                            src={audioUrl}
                            controls
                            className="w-full"
                        />
                    </div>
                    <div className="text-sm text-gray-400 text-center">
                        Duration: {formatTime(recordingTime)}
                    </div>
                    <div className="flex justify-between space-x-2">
                        <button
                            onClick={discardRecording}
                            className="flex-1 py-1.5 px-3 flex items-center justify-center bg-red-600 hover:bg-red-700 text-white rounded-lg transition-colors"
                        >
                            <TrashIcon className="w-4 h-4 mr-1" />
                            Discard
                        </button>
                        <button
                            onClick={saveRecording}
                            className="flex-1 py-1.5 px-3 flex items-center justify-center bg-green-600 hover:bg-green-700 text-white rounded-lg transition-colors"
                        >
                            <CheckIcon className="w-4 h-4 mr-1" />
                            Add to Project
                        </button>
                    </div>
                </div>
            )}
        </div>
    );
});

export default AudioRecorder;