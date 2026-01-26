import React from 'react';
import Modal from 'react-modal';
import { observer } from 'mobx-react-lite';
import dynamic from 'next/dynamic';
import { StateContext } from '../../../states';

const ProjectLoadingModal = observer(() => {
  const state = React.useContext(StateContext);

  const handleCloseModal = () => {
    state.setProjectLoadingStatus('idle', '');
    state.setProjectLoadingProgress(0);
  };

  return (
    <Modal
      isOpen={state.projectLoadingStatus !== 'idle'}
      onRequestClose={() => {
        // Only allow closing if we're not in the 'loading' state
        if (state.projectLoadingStatus !== 'loading') {
          handleCloseModal();
        }
      }}
      contentLabel="Project Loading Modal"
      className="fixed inset-0 flex items-center justify-center z-50 bg-black/50"
      overlayClassName="fixed inset-0 z-40 bg-black/60"
      shouldCloseOnOverlayClick={state.projectLoadingStatus !== 'loading'}
    >
      <div className="bg-white dark:bg-gray-800 p-6 rounded-sm shadow-lg max-w-md mx-auto">
        {state.projectLoadingStatus === 'loading' && (
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
              <p className="text-gray-700 dark:text-gray-300">
                {state.projectLoadingMessage || 'Please wait while your project is being loaded...'}
              </p>

              {/* Progress bar */}
              {state.projectLoadingProgress > 0 && (
                <div className="mt-3">
                  <div className="w-full bg-gray-200 rounded-full h-2.5">
                    <div
                      className="bg-purple-600 h-2.5 rounded-full"
                      style={{ width: `${state.projectLoadingProgress}%` }}
                    ></div>
                  </div>
                  <p className="text-xs text-gray-500 dark:text-gray-400 text-right mt-1">
                    {state.projectLoadingProgress}%
                  </p>
                </div>
              )}
            </div>
          </div>
        )}

        {state.projectLoadingStatus === 'success' && (
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
              <h2 className="text-xl font-bold text-purple-600 dark:text-purple-400">
                Project Loaded Successfully!
              </h2>
              <p className="text-gray-700 dark:text-gray-300">Your project is now ready to use.</p>
              <button
                className="mt-4 bg-purple-500 hover:bg-purple-700 text-white font-bold py-2 px-4 rounded-sm"
                onClick={handleCloseModal}
              >
                Close
              </button>
            </div>
          </div>
        )}

        {state.projectLoadingStatus === 'error' && (
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
              <p className="text-red-600 dark:text-red-400 font-medium mt-2 p-2 bg-red-100 dark:bg-red-900 rounded-sm">
                {state.projectLoadingMessage || 'Unknown error'}
              </p>
              <button
                className="mt-4 bg-red-500 hover:bg-red-700 text-white font-bold py-2 px-4 rounded-sm"
                onClick={handleCloseModal}
              >
                Close
              </button>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
});

export default dynamic(() => Promise.resolve(ProjectLoadingModal), { ssr: false });
export { ProjectLoadingModal };
