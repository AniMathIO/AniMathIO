import React from 'react';
import Head from 'next/head';
import Link from 'next/link';
import Image from 'next/image';

export default function HomePage() {
  const [log, setLog] = React.useState('');
  const [value, setValue] = React.useState('5');
  const [message, setMessage] = React.useState('');

  const handleChange = (event: any) => {
    setValue(event.target.value);
    setMessage('');
  };

  React.useEffect(() => {
    window.ipc.on('log', (...args: unknown[]) => {
      const log = args[0] as string;
      setLog(log);
    });
    window.ipc.on('message', (...args: unknown[]) => {
      const msg = args[0] as string;
      setMessage(msg);
    });
  }, []);

  return (
    <React.Fragment>
      <Head>
        <title>ManimStudioNext</title>
      </Head>
      <div className="container mx-auto p-4">
        <div className="text-center mb-8">
          <Image
            className="mx-auto"
            src="/images/ManimStudioLogoDark.png"
            alt="Logo image"
            width="256"
            height="256"
          />
          <h1 className="text-4xl font-bold mb-4">ManimStudioNext</h1>
        </div>
        <div className="bg-white shadow-md rounded-lg p-6">
          <div className="mb-4">
            <label htmlFor="numberInput" className="block text-gray-700 font-bold mb-2">
              Calculate the square of your number:
            </label>
            <div className="flex items-center">
              <input
                id="numberInput"
                type="number"
                value={value}
                onChange={handleChange}
                aria-label="Number input"
                placeholder="Enter a number"
                className="border border-gray-300 rounded-l px-4 py-2 w-full focus:outline-none focus:ring-2 focus:ring-blue-500 text-gray-700"
              />
              <button
                onClick={() => {
                  console.log('DEBUG: running the Python script via IPC');
                  window.ipc.send('run-sh', value);
                }}
                className="bg-blue-500 hover:bg-blue-600 text-white font-bold py-2 px-4 rounded-r"
              >
                Calculate
              </button>
            </div>
          </div>
          <div className="mb-4">
            <p className="text-gray-700">{log}</p>
            <p className="text-gray-700">
              The square of {value} is {'-> '} {message}
            </p>
          </div>
          <div className="text-center">
            <Link href="/next" className='bg-blue-500 hover:bg-blue-600 text-white font-bold py-2 px-4 rounded'>
              Go to next page
            </Link>
          </div>
        </div>
      </div>
    </React.Fragment>
  );
}