"use client";
import React, { useState, useRef, useEffect } from "react";
import { StateContext } from "@/states";
import { observer } from "mobx-react";
import { MicrophoneIcon, StopIcon, TrashIcon, CheckIcon, AdjustmentsHorizontalIcon } from "@heroicons/react/24/solid";

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

    const mediaRecorderRef = useRef<MediaRecorder | null>(null);
    const audioChunksRef = useRef<Blob[]>([]);
    const requestAnimationRef = useRef<number | null>(null);
    const audioContextRef = useRef<AudioContext | null>(null);
    const analyserRef = useRef<AnalyserNode | null>(null);
    const streamRef = useRef<MediaStream | null>(null);
    const startTimeRef = useRef<number>(0);

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

            console.log("Available audio devices:", audioInputs);
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

    // Listen for device changes
    useEffect(() => {
        getAudioDevices();

        // Set up a listener for device changes
        navigator.mediaDevices.addEventListener('devicechange', getAudioDevices);

        return () => {
            navigator.mediaDevices.removeEventListener('devicechange', getAudioDevices);
        };
    }, []);

    // Recording timer effect
    useEffect(() => {
        let interval: NodeJS.Timeout | null = null;

        if (isRecording && !isPaused) {
            interval = setInterval(() => {
                setRecordingTime((prev) => prev + 1);
            }, 1000);
        }

        return () => {
            if (interval) clearInterval(interval);
        };
    }, [isRecording, isPaused]);

    // Check if we're running in Electron
    const isElectron = () => {
        // @ts-ignore
        return window?.electron !== undefined;
    };

    // Visualizer animation frame
    const updateVisualizer = () => {
        if (!analyserRef.current) return;

        const dataArray = new Uint8Array(analyserRef.current.frequencyBinCount);
        analyserRef.current.getByteFrequencyData(dataArray);

        // Reduce the data to a manageable size for visualization
        const visualData = Array.from({ length: 30 }, (_, i) => {
            const start = Math.floor(i * dataArray.length / 30);
            const end = Math.floor((i + 1) * dataArray.length / 30);
            let sum = 0;
            for (let j = start; j < end; j++) {
                sum += dataArray[j];
            }
            return sum / (end - start); // Average value
        });

        setVisualizerData(visualData);

        if (isRecording && !isPaused) {
            requestAnimationRef.current = requestAnimationFrame(updateVisualizer);
        }
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

        try {
            // First check if we need to request Electron-specific permissions
            const permissionGranted = await requestElectronPermission();
            if (!permissionGranted) {
                setPermissionError("Microphone access denied. Please enable microphone access in your system settings.");
                return;
            }

            // Additional logging for debugging
            console.log("Starting recording, requesting user media with device:", selectedDeviceId);

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

            // Standard getUserMedia request with additional constraints
            const stream = await navigator.mediaDevices.getUserMedia({
                audio: audioConstraints
            });

            console.log("Media stream obtained successfully");
            streamRef.current = stream;

            // Check if we actually got audio tracks
            if (stream.getAudioTracks().length === 0) {
                throw new Error("No audio tracks found in the media stream");
            }

            // Log audio track info for debugging
            const audioTrack = stream.getAudioTracks()[0];
            console.log("Audio track:", audioTrack.label, audioTrack.enabled, audioTrack.readyState);

            // Set up audio context and analyser for visualization
            // @ts-ignore - AudioContext might not be recognized in some TS configs
            const AudioContextClass = window.AudioContext || window.webkitAudioContext;
            const audioContext = new AudioContextClass();
            audioContextRef.current = audioContext;

            const source = audioContext.createMediaStreamSource(stream);
            const analyser = audioContext.createAnalyser();
            analyserRef.current = analyser;

            analyser.fftSize = 2048;
            source.connect(analyser);

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
                    console.log("Using MIME type:", selectedMimeType);
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
                console.log("Data available event, size:", event.data.size);
                if (event.data.size > 0) {
                    audioChunksRef.current.push(event.data);
                }
            };

            mediaRecorder.onstop = () => {
                console.log("MediaRecorder stopped, chunks:", audioChunksRef.current.length);
                if (audioChunksRef.current.length === 0) {
                    setPermissionError("No audio data was recorded. Please check your microphone settings.");
                    return;
                }

                const mimeType = mediaRecorder.mimeType || 'audio/webm';
                console.log("Creating blob with MIME type:", mimeType);
                const audioBlob = new Blob(audioChunksRef.current, { type: mimeType });
                console.log("Blob created, size:", audioBlob.size);

                const url = URL.createObjectURL(audioBlob);
                setAudioUrl(url);

                // Clean up stream tracks
                if (streamRef.current) {
                    streamRef.current.getTracks().forEach(track => {
                        console.log("Stopping track:", track.label);
                        track.stop();
                    });
                }
            };

            audioChunksRef.current = [];

            // Request data at regular intervals for more reliable recording
            console.log("Starting MediaRecorder");
            mediaRecorder.start(100);
            setIsRecording(true);
            startTimeRef.current = Date.now();

            // Start the visualizer
            requestAnimationRef.current = requestAnimationFrame(updateVisualizer);

        } catch (error) {
            console.error("Error accessing microphone:", error);

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
                // Some versions of MediaRecorder don't support pause
                try {
                    mediaRecorderRef.current.pause();
                    console.log("MediaRecorder paused");
                } catch (e) {
                    console.warn("MediaRecorder pause not supported", e);
                    // If pause is not supported, we'll just stop the recording
                    stopRecording();
                    return;
                }

                setIsPaused(true);

                // Cancel animation frame
                if (requestAnimationRef.current) {
                    cancelAnimationFrame(requestAnimationRef.current);
                    requestAnimationRef.current = null;
                }
            } else {
                try {
                    mediaRecorderRef.current.resume();
                    console.log("MediaRecorder resumed");
                } catch (e) {
                    console.warn("MediaRecorder resume not supported", e);
                    // If resume is not supported, we might need to restart recording
                    // This is a complex case and might require a different approach
                }

                setIsPaused(false);

                // Restart animation
                requestAnimationRef.current = requestAnimationFrame(updateVisualizer);
            }
        }
    };

    const stopRecording = () => {
        if (mediaRecorderRef.current && isRecording) {
            try {
                console.log("Stopping MediaRecorder");
                mediaRecorderRef.current.stop();
            } catch (e) {
                console.error("Error stopping recorder:", e);
                // Even if there's an error, we should update the UI
            }

            setIsRecording(false);
            setIsPaused(false);

            // Cancel animation frame
            if (requestAnimationRef.current) {
                cancelAnimationFrame(requestAnimationRef.current);
                requestAnimationRef.current = null;
            }
        }
    };

    const discardRecording = () => {
        if (audioUrl) {
            console.log("Discarding recording");
            URL.revokeObjectURL(audioUrl);
            setAudioUrl(null);
        }

        setRecordingTime(0);
        audioChunksRef.current = [];
    };

    const saveRecording = () => {
        if (audioUrl) {
            console.log("Saving recording");

            // Add to the audio resources
            state.addAudioResource(audioUrl);

            // Find the index of the newly added audio
            const index = state.audios.length - 1;
            console.log("Audio added at index:", index);

            // Create a new audio element to preload the audio
            const preloadAudio = new Audio(audioUrl);
            preloadAudio.addEventListener('loadedmetadata', () => {
                console.log("Audio metadata loaded, duration:", preloadAudio.duration);

                // Now we know the audio is loaded, add it to the timeline
                state.addAudio(index);

                // Reset after saving
                setAudioUrl(null);
                setRecordingTime(0);
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
        <div className="p-4 bg-gray-800 rounded-lg mb-4">
            <div className="flex justify-between items-center mb-2">
                <h3 className="text-sm font-semibold">Voice Narration</h3>

                {!isRecording && !audioUrl && audioDevices.length > 0 && (
                    <button
                        onClick={toggleDeviceSelector}
                        className="text-gray-400 hover:text-white transition-colors"
                        title="Select microphone"
                    >
                        <AdjustmentsHorizontalIcon className="w-5 h-5" />
                    </button>
                )}
            </div>

            {showDeviceSelector && !isRecording && !audioUrl && (
                <div className="mb-3 p-2 bg-gray-700 rounded-lg">
                    <label className="block text-xs text-gray-400 mb-1">Select Microphone</label>
                    <select
                        value={selectedDeviceId}
                        onChange={(e) => setSelectedDeviceId(e.target.value)}
                        className="w-full bg-gray-900 text-white rounded py-1.5 px-2 text-sm"
                    >
                        {audioDevices.map((device) => (
                            <option key={device.deviceId} value={device.deviceId}>
                                {formatDeviceName(device)}
                            </option>
                        ))}
                    </select>
                    <button
                        onClick={getAudioDevices}
                        className="mt-2 text-xs text-blue-400 hover:text-blue-300"
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

                    <div className="h-16 bg-gray-900 rounded-lg overflow-hidden flex items-end p-1">
                        {visualizerData.length > 0 ? (
                            visualizerData.map((value, index) => (
                                <div
                                    key={index}
                                    className="w-1 mx-0.5 bg-gradient-to-t from-red-600 to-red-400"
                                    style={{
                                        height: `${Math.min(100, value / 2)}%`,
                                        transition: 'height 0.1s ease'
                                    }}
                                ></div>
                            ))
                        ) : (
                            <div className="w-full text-center text-gray-500 text-xs">
                                {isPaused ? "Recording paused" : "Speak now..."}
                            </div>
                        )}
                    </div>

                    <div className="flex justify-between space-x-2">
                        <button
                            onClick={pauseRecording}
                            className={`flex-1 py-1.5 px-3 flex items-center justify-center ${isPaused ? 'bg-green-600 hover:bg-green-700' : 'bg-yellow-600 hover:bg-yellow-700'
                                } text-white rounded-lg transition-colors`}
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