export interface ProjectHistoryItem {
  id?: number;
  filePath: string;
  fileName: string;
  lastUpdate: number; // timestamp
}

const DB_NAME = "AniMathIOProjects";
const DB_VERSION = 1;
const STORE_NAME = "projects";

// Initialize IndexedDB
function openDB(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onerror = () => reject(request.error);
    request.onsuccess = () => resolve(request.result);

    request.onupgradeneeded = (event) => {
      const db = (event.target as IDBOpenDBRequest).result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        const objectStore = db.createObjectStore(STORE_NAME, {
          keyPath: "id",
          autoIncrement: true,
        });
        objectStore.createIndex("filePath", "filePath", { unique: false });
        objectStore.createIndex("lastUpdate", "lastUpdate", { unique: false });
        objectStore.createIndex("fileName", "fileName", { unique: false });
      }
    };
  });
}

// Add or update a project in history
export async function addProjectToHistory(
  filePath: string,
  fileName: string
): Promise<void> {
  try {
    const db = await openDB();
    const transaction = db.transaction([STORE_NAME], "readwrite");
    const store = transaction.objectStore(STORE_NAME);
    const index = store.index("filePath");

    // Check if project already exists
    const existingRequest = index.get(filePath);
    await new Promise<void>((resolve, reject) => {
      existingRequest.onsuccess = () => {
        const existing = existingRequest.result;
        if (existing) {
          // Update existing project
          existing.lastUpdate = Date.now();
          existing.fileName = fileName;
          const updateRequest = store.put(existing);
          updateRequest.onsuccess = () => resolve();
          updateRequest.onerror = () => reject(updateRequest.error);
        } else {
          // Add new project
          const newProject: ProjectHistoryItem = {
            filePath,
            fileName,
            lastUpdate: Date.now(),
          };
          const addRequest = store.add(newProject);
          addRequest.onsuccess = () => resolve();
          addRequest.onerror = () => reject(addRequest.error);
        }
      };
      existingRequest.onerror = () => reject(existingRequest.error);
    });
  } catch (error) {
    console.error("Error adding project to history:", error);
    throw error;
  }
}

// Get all projects from history
export async function getAllProjects(): Promise<ProjectHistoryItem[]> {
  try {
    const db = await openDB();
    const transaction = db.transaction([STORE_NAME], "readonly");
    const store = transaction.objectStore(STORE_NAME);
    const request = store.getAll();

    return new Promise((resolve, reject) => {
      request.onsuccess = () => resolve(request.result || []);
      request.onerror = () => reject(request.error);
    });
  } catch (error) {
    console.error("Error getting projects:", error);
    return [];
  }
}

// Delete a project from history
export async function deleteProjectFromHistory(id: number): Promise<void> {
  try {
    const db = await openDB();
    const transaction = db.transaction([STORE_NAME], "readwrite");
    const store = transaction.objectStore(STORE_NAME);
    const request = store.delete(id);

    return new Promise((resolve, reject) => {
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  } catch (error) {
    console.error("Error deleting project from history:", error);
    throw error;
  }
}

// Get project by file path
export async function getProjectByPath(
  filePath: string
): Promise<ProjectHistoryItem | null> {
  try {
    const db = await openDB();
    const transaction = db.transaction([STORE_NAME], "readonly");
    const store = transaction.objectStore(STORE_NAME);
    const index = store.index("filePath");
    const request = index.get(filePath);

    return new Promise((resolve, reject) => {
      request.onsuccess = () => resolve(request.result || null);
      request.onerror = () => reject(request.error);
    });
  } catch (error) {
    console.error("Error getting project by path:", error);
    return null;
  }
}
