import { useEffect, useRef } from "react";
import type { ThemeFile } from "@/lib/ailight";
import { blendRgb, type Rgb, sampleLedTrack } from "@/lib/light-simulator";
import { cn } from "@/lib/utils";

const BLACK: Rgb = [0, 0, 0];

function renderLight(
  el: HTMLDivElement | null,
  rgb: Rgb,
  brightness: number | null
) {
  if (!el) {
    return;
  }
  const color = `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`;
  el.style.backgroundColor = color;
  el.style.boxShadow =
    brightness === null
      ? "none"
      : `0 0 ${Math.max(8, brightness * 0.45)}px ${color}`;
  el.style.opacity = brightness === null ? "0.1" : "1";
}

function renderStaticScene(
  scene: ThemeFile["scenes"][string] | null,
  lights: Array<HTMLDivElement | null>,
  elapsedMs: number
) {
  for (let index = 0; index < 3; index += 1) {
    const track = scene?.leds[index] ?? null;
    renderLight(
      lights[index],
      sampleLedTrack(track, elapsedMs) ?? BLACK,
      track?.brightness ?? null
    );
  }
}

interface SceneLightPreviewProps {
  className?: string;
  orientation?: "horizontal" | "vertical";
  scene: ThemeFile["scenes"][string] | null;
  transitionMs?: number;
}

export function SceneLightPreview({
  className,
  orientation = "vertical",
  scene,
  transitionMs = 0,
}: SceneLightPreviewProps) {
  const lightRefs = useRef<Array<HTMLDivElement | null>>([]);
  const sceneRef = useRef(scene);
  const epochRef = useRef(0);
  const transitionRef = useRef({ duration: 0, from: [BLACK, BLACK, BLACK] });
  const displayedRef = useRef<Rgb[]>([BLACK, BLACK, BLACK]);
  const initializedRef = useRef(false);

  useEffect(() => {
    const now = performance.now();
    transitionRef.current = {
      duration:
        initializedRef.current && sceneRef.current !== null ? transitionMs : 0,
      from: displayedRef.current.map((rgb) => [...rgb] as Rgb),
    };
    sceneRef.current = scene;
    epochRef.current = now;
    initializedRef.current = scene !== null;
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      renderStaticScene(scene, lightRefs.current, transitionMs);
    }
  }, [scene, transitionMs]);

  useEffect(() => {
    const reduceMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)"
    ).matches;

    const renderFrame = (now: number) => {
      const elapsed = Math.max(0, now - epochRef.current);
      const transition = transitionRef.current;
      const progress =
        transition.duration > 0 ? elapsed / transition.duration : 1;

      for (let index = 0; index < 3; index += 1) {
        const track = sceneRef.current?.leds[index] ?? null;
        const sampled = sampleLedTrack(track, elapsed) ?? BLACK;
        const rgb = blendRgb(
          transition.from[index] ?? BLACK,
          sampled,
          progress
        );
        displayedRef.current[index] = rgb;
        renderLight(lightRefs.current[index], rgb, track?.brightness ?? null);
      }
    };

    const tick = (now: number) => {
      renderFrame(now);
      raf = requestAnimationFrame(tick);
    };

    let raf = 0;
    if (!reduceMotion) {
      raf = requestAnimationFrame(tick);
    }
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <div
      aria-label="当前主题三灯模拟"
      className={cn(
        "scene-light-preview",
        `scene-light-preview--${orientation}`,
        className
      )}
      role="img"
    >
      {["顶灯", "中灯", "底灯"].map((label, index) => (
        <div className="scene-light-preview__track" key={label}>
          <span>{label}</span>
          <div
            className="scene-light-preview__light"
            ref={(node) => {
              lightRefs.current[index] = node;
            }}
          />
        </div>
      ))}
    </div>
  );
}
