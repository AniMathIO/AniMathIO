import React, { MouseEventHandler, useEffect, useRef, useMemo } from "react";

function debounce<A extends unknown[]>(func: (...args: A) => void, wait: number) {
    let timeout: NodeJS.Timeout;
    return (...args: A) => {
        clearTimeout(timeout);
        timeout = setTimeout(() => func(...args), wait);
    };
}

function Dragable(props: {
    children?: React.ReactNode;
    disabled?: boolean;
    className?: string;
    style?: React.CSSProperties;
    value: number;
    total: number;
    onChange: (value: number) => void;
}) {
    const ref = useRef<{
        div: HTMLDivElement | null;
        isDragging: boolean;
        initialMouseX: number;
    }>({
        div: null,
        isDragging: false,
        initialMouseX: 0,
    });
    const { current: data } = ref;

    const debouncedOnChange = useMemo(() => debounce(props.onChange, 100), [props.onChange]);

    function calculateNewValue(mouseX: number): number {
        if (!data.div) return 0;
        const deltaX = mouseX - data.initialMouseX;
        const deltaValue =
            (deltaX / data.div.parentElement!.clientWidth) * props.total;
        return props.value + deltaValue;
    }

    const handleMouseDown: MouseEventHandler<HTMLDivElement> = (event) => {
        if (!data.div) return;
        if (props.disabled) return;
        data.isDragging = true;
        data.initialMouseX = event.clientX;
    };
    const handleMouseMove: MouseEventHandler<HTMLDivElement> = (event) => {
        if (!data.div) return;
        if (!data.isDragging) return;
        data.div.style.left = `${(calculateNewValue(event.clientX) / props.total) * 100}%`;
        event.stopPropagation();
        event.preventDefault();
    };

    const handleMouseUp: MouseEventHandler<HTMLDivElement> = (event) => {
        // console.log("mouse up");
        if (!data.div) return;
        if (!data.isDragging) return;
        data.isDragging = false;
        debouncedOnChange(calculateNewValue(event.clientX));
        event.stopPropagation();
        event.preventDefault();
    };

    useEffect(() => {
        window.addEventListener("mouseup", handleMouseUp as unknown as EventListener);
        window.addEventListener("mousemove", handleMouseMove as unknown as EventListener);
        return () => {
            window.removeEventListener("mouseup", handleMouseUp as unknown as EventListener);
            window.removeEventListener("mousemove", handleMouseMove as unknown as EventListener);
        };
    }, [handleMouseUp, handleMouseMove]);

    return (
        <div
            ref={(r) => {
                data.div = r;
            }}
            className={`absolute height-100 ${props.className}`}
            style={{
                left: (props.value / props.total) * 100 + "%",
                top: 0,
                bottom: 0,
                ...props.style,
            }}
            onMouseDown={handleMouseDown}
        >
            {props.children}
        </div>
    );
}

export default Dragable;
