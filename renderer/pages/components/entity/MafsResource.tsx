import React from "react";
import { StateContext } from "../../states";
import { observer } from "mobx-react";
import { PlusCircleIcon } from "@heroicons/react/24/solid";
import { Mafs } from "mafs";
import { createRoot } from "react-dom/client";
import ReactDOM from "react-dom";

type MafsResourceProps = {
  index: number;
  children: React.ReactNode;
  name: string;
};

function extractSVG(mafsElement: React.ReactNode): Promise<string> {
  return new Promise((resolve, reject) => {
    const container = document.createElement('div');
    // Set dimensions to ensure that the SVG has space to render properly
    container.style.position = 'absolute';
    container.style.top = '-9999px'; // Move off-screen without hiding
    container.style.width = '1000px'; // Assume sufficient width for rendering
    container.style.height = '1000px'; // Assume sufficient height for rendering

    document.body.appendChild(container);

    const root = createRoot(container);
    root.render(
      <div ref={el => {
        if (el) {
          // Wait long enough for all elements to be rendered and styled
          setTimeout(() => {
            const svg = el.querySelector('svg');
            console.log(svg);
            if (svg) {
              resolve(svg.outerHTML);
            } else {
              reject('SVG not found in Mafs component');
            }
            root.unmount();
            document.body.removeChild(container);
          }, 1000); // Increase timeout to ensure everything is rendered
        }
      }}>
        {mafsElement}
      </div>
    );
  });
}


export const MafsResource = observer(({ index, children, name }: MafsResourceProps) => {
  const state = React.useContext(StateContext);
  const containerRef = React.useRef(null);

  const handleAddResource = async () => {
    try {
      const svgContent = await extractSVG(children);
      state.addMafsResource(index, svgContent, name);
    } catch (error) {
      console.error('Failed to extract SVG from Mafs component:', error);
    }
  };

  function extractSVG(mafsElement: React.ReactNode): Promise<string> {
    return new Promise((resolve, reject) => {
      const container = document.createElement('div');
      container.style.display = 'none';
      document.body.appendChild(container);

      const root = createRoot(container);
      root.render(
        <div ref={el => {
          if (el) {
            // Use requestAnimationFrame to wait for the next animation frame
            requestAnimationFrame(() => {
              const svg = el.querySelector('svg');
              if (svg) {
                resolve(svg.outerHTML);
              } else {
                reject('SVG not found in Mafs component');
              }
              root.unmount();
              document.body.removeChild(container);
            });
          }
        }}>
          {mafsElement}
        </div>
      );
    });
  }

  return (
    <div className="rounded-lg overflow-hidden items-center bg-slate-800 m-[15px] flex flex-col relative">
      <div className="bg-[rgba(0,0,0,.50)] rounded-xl text-white py-1 px-1 absolute text-base top-1.3 right-2">
        {name}
      </div>
      <button
        title="Add Image"
        className="hover:bg-[#00a0f5] rounded z-10 text-white font-bold py-1 absolute text-lg bottom-2 right-2"
        onClick={handleAddResource}
      >
        <PlusCircleIcon className="w-8 h-8 drop-shadow-lg" />
      </button>
      <div ref={containerRef} className="w-full flex justify-center">
        <div
          className="min-h-[130px] min-w-[200px] max-h-[130px] max-w-[300px] object-scale-down">
          {children}
        </div>
      </div>
    </div>
  );
});