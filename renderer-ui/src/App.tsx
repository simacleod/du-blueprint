import { ChangeEvent, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import SceneCanvas from "./components/SceneCanvas";
import type { BlueprintData, BlueprintResponse, LodData } from "./api";
import { decodeBlueprint } from "./api";

const formatVector = ([x, y, z]: [number, number, number]) =>
  `(${x.toFixed(2)}, ${y.toFixed(2)}, ${z.toFixed(2)})`;

const App = () => {
  const [blueprint, setBlueprint] = useState<BlueprintData | null>(null);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showBounds, setShowBounds] = useState(true);
  const [lastPath, setLastPath] = useState<string | null>(null);

  const availableLods = blueprint?.lods ?? [];
  const activeLod: LodData | null = useMemo(() => {
    if (!availableLods.length) {
      return null;
    }
    return availableLods[Math.min(selectedIndex, availableLods.length - 1)];
  }, [availableLods, selectedIndex]);

  const loadBlueprint = useCallback(async (path: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<BlueprintResponse>("load_blueprint", { path });
      setBlueprint(decodeBlueprint(result));
      setSelectedIndex(0);
      setLastPath(path);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!blueprint) {
      return;
    }
    if (selectedIndex < blueprint.lods.length) {
      return;
    }
    setSelectedIndex(0);
  }, [blueprint, selectedIndex]);

  useEffect(() => {
    (async () => {
      try {
        const initialPath = await invoke<string | null>("initial_blueprint");
        if (initialPath) {
          await loadBlueprint(initialPath);
        }
      } catch (err) {
        console.warn("Failed to fetch initial blueprint", err);
      }
    })();
  }, [loadBlueprint]);

  const handleOpenBlueprint = useCallback(async () => {
    try {
      const selection = await open({
        multiple: false,
        filters: [
          { name: "Blueprints", extensions: ["blueprint", "json"] },
          { name: "All Files", extensions: ["*"] }
        ]
      });

      if (!selection) {
        return;
      }

      if (Array.isArray(selection)) {
        const first = selection[0];
        if (typeof first === "string") {
          await loadBlueprint(first);
        }
        return;
      }

      if (typeof selection === "string") {
        await loadBlueprint(selection);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [loadBlueprint]);

  const handleSliderChange = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setSelectedIndex(Number(event.target.value));
  }, []);

  const lodSummary = useMemo(() => {
    if (!availableLods.length) {
      return [] as { label: string; count: number }[];
    }
    return availableLods.map((lod) => ({
      label: `LOD ${lod.level}`,
      count: lod.voxelCount
    }));
  }, [availableLods]);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <h1>DU Blueprint Renderer</h1>
        <section>
          <button onClick={handleOpenBlueprint} disabled={loading}>
            {loading ? "Loading..." : "Open Blueprint"}
          </button>
          {lastPath && (
            <span className="metadata" title={lastPath}>
              Active: {lastPath}
            </span>
          )}
          {error && <span className="error">{error}</span>}
        </section>

        {blueprint ? (
          <>
            <section className="metadata">
              <strong>{blueprint.model.name}</strong>
              <span>Voxel Count: {activeLod?.voxelCount ?? 0}</span>
              <span>Model Size: {blueprint.model.size}</span>
              <span>
                Bounds: {formatVector(blueprint.model.bounds.min)} → {" "}
                {formatVector(blueprint.model.bounds.max)}
              </span>
            </section>

            {availableLods.length > 0 && (
              <section>
                <label htmlFor="lod-slider">Level of Detail</label>
                <input
                  id="lod-slider"
                  type="range"
                  min={0}
                  max={availableLods.length - 1}
                  value={Math.min(selectedIndex, Math.max(availableLods.length - 1, 0))}
                  onChange={handleSliderChange}
                />
                <span className="metadata">
                  Viewing LOD {activeLod?.level ?? "–"} ({activeLod?.voxelCount ?? 0} voxels)
                </span>
              </section>
            )}

            <section className="toggle">
              <input
                id="show-bounds"
                type="checkbox"
                checked={showBounds}
                onChange={(event) => setShowBounds(event.target.checked)}
              />
              <label htmlFor="show-bounds">Show Bounds</label>
            </section>

            {lodSummary.length > 0 && (
              <section className="metadata">
                <strong>LOD Breakdown</strong>
                {lodSummary.map((lod) => (
                  <span key={lod.label}>
                    {lod.label}: {lod.count} voxels
                  </span>
                ))}
              </section>
            )}
          </>
        ) : (
          <section className="metadata">
            <span>Select a blueprint to begin rendering.</span>
          </section>
        )}
      </aside>

      <main className="scene-container">
        <SceneCanvas blueprint={blueprint} activeLod={activeLod} showBounds={showBounds} />
        {loading && <div className="status-banner">Decoding blueprint...</div>}
      </main>
    </div>
  );
};

export default App;
