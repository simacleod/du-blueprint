import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { BlueprintData, LodData } from "../api";

interface SceneCanvasProps {
  blueprint: BlueprintData | null;
  activeLod: LodData | null;
  showBounds: boolean;
}




const MAX_RENDER_PIXELS = 2_000_000;
const MIN_RENDER_RATIO = 0.45;

const computePixelRatio = (width: number, height: number) => {
  if (width === 0 || height === 0) {
    return 1;
  }

  const deviceRatio = Math.min(window.devicePixelRatio || 1, 1);
  const devicePixels = width * height * deviceRatio * deviceRatio;

  if (devicePixels <= MAX_RENDER_PIXELS) {
    return deviceRatio;
  }

  const scaledRatio = Math.sqrt(MAX_RENDER_PIXELS / (width * height));
  return Math.max(MIN_RENDER_RATIO, Math.min(deviceRatio, scaledRatio));
};

const SceneCanvas = ({ blueprint, activeLod, showBounds }: SceneCanvasProps) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const rendererRef = useRef<THREE.WebGLRenderer | null>(null);
  const sceneRef = useRef<THREE.Scene | null>(null);
  const cameraRef = useRef<THREE.PerspectiveCamera | null>(null);
  const controlsRef = useRef<OrbitControls | null>(null);
  const frameRef = useRef<number>();
  const meshRef = useRef<THREE.Mesh | null>(null);
  const boundsRef = useRef<THREE.LineSegments | null>(null);
  const needsRenderRef = useRef(false);
  const isAnimatingRef = useRef(false);
  const renderRef = useRef<() => void>(() => {});

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const contextAttributes: WebGLContextAttributes & { desynchronized?: boolean } = {
      antialias: false,
      alpha: false,
      depth: true,
      powerPreference: "high-performance",
      desynchronized: true
    };

    const canvas = document.createElement("canvas");
    const context =
      canvas.getContext("webgl2", contextAttributes) ??
      canvas.getContext("webgl", contextAttributes) ??
      canvas.getContext("experimental-webgl", contextAttributes);

    const renderer = new THREE.WebGLRenderer({
      canvas,
      context: context ?? undefined,
      antialias: false,
      alpha: false,
      powerPreference: "high-performance"
    });
    renderer.domElement.style.width = "100%";
    renderer.domElement.style.height = "100%";
    renderer.sortObjects = false;
    renderer.shadowMap.enabled = false;
    const pixelRatio = computePixelRatio(container.clientWidth, container.clientHeight);
    renderer.setPixelRatio(pixelRatio);
    renderer.setSize(container.clientWidth, container.clientHeight);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    const gl = renderer.getContext();
    if (gl) {
      gl.disable(gl.DITHER);
    }
    container.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0f172a);

    const info = gl.getExtension('WEBGL_debug_renderer_info');
    if (info) {
      console.log(
        'GL_VENDOR:', gl.getParameter(info.UNMASKED_VENDOR_WEBGL),
        'GL_RENDERER:', gl.getParameter(info.UNMASKED_RENDERER_WEBGL)
      );
      console.log('UNMASKED_VENDOR_WEBGL:', gl.getParameter(info.UNMASKED_VENDOR_WEBGL));
      console.log('UNMASKED_RENDERER_WEBGL:', gl.getParameter(info.UNMASKED_RENDERER_WEBGL));
    }
    console.log('WebGL VERSION:', gl.getParameter(gl.VERSION));
    console.log('UA:', navigator.userAgent);
    
    // 2) How big is the backbuffer? (to rule out accidental 8K)
    console.log('drawingBuffer:', gl.drawingBufferWidth + 'x' + gl.drawingBufferHeight);
    
    // 3) Are we software-timing the GPU? (GPU timer query)
    const hasTQ = !!gl.getExtension('EXT_disjoint_timer_query') || !!gl.getExtension('EXT_disjoint_timer_query_webgl2');
    console.log('has timer query:', hasTQ);

    const camera = new THREE.PerspectiveCamera(
      60,
      container.clientWidth / container.clientHeight,
      0.1,
      2000
    );
    camera.position.set(8, 8, 8);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.1;

    rendererRef.current = renderer;
    sceneRef.current = scene;
    cameraRef.current = camera;
    controlsRef.current = controls;

    const renderScene = () => {
      renderer.render(scene, camera);
    };
    renderRef.current = renderScene;

    const animate = () => {
      if (!isAnimatingRef.current) {
        return;
      }

      frameRef.current = requestAnimationFrame(animate);
      const needsUpdate = controls.update();
      if (needsUpdate || needsRenderRef.current) {
        needsRenderRef.current = false;
        renderScene();
      }
    };

    needsRenderRef.current = true;
    isAnimatingRef.current = true;
    animate();

    const handleResize = () => {
      const currentContainer = containerRef.current;
      const currentRenderer = rendererRef.current;
      const currentCamera = cameraRef.current;
      if (!currentContainer || !currentRenderer || !currentCamera) {
        return;
      }

      const { clientWidth, clientHeight } = currentContainer;
      const ratio = computePixelRatio(clientWidth, clientHeight);
      currentRenderer.setPixelRatio(ratio);
      currentRenderer.setSize(clientWidth, clientHeight);
      currentCamera.aspect = clientWidth / clientHeight;
      currentCamera.updateProjectionMatrix();
      needsRenderRef.current = true;
    };

    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);

      if (frameRef.current) {
        cancelAnimationFrame(frameRef.current);
      }

      isAnimatingRef.current = false;

      controls.dispose();
      renderer.dispose();

      if (container.contains(renderer.domElement)) {
        container.removeChild(renderer.domElement);
      }

      rendererRef.current = null;
      sceneRef.current = null;
      cameraRef.current = null;
      controlsRef.current = null;
      renderRef.current = () => {};
    };
  }, []);

  useEffect(() => {
    const scene = sceneRef.current;
    if (!scene) {
      return;
    }

    if (boundsRef.current) {
      scene.remove(boundsRef.current);
      boundsRef.current.geometry.dispose();
      const material = boundsRef.current.material;
      if (Array.isArray(material)) {
        material.forEach((m) => m.dispose());
      } else {
        material.dispose();
      }
      boundsRef.current = null;
      needsRenderRef.current = true;
    }

    if (!blueprint) {
      return;
    }

    const [minX, minY, minZ] = blueprint.model.bounds.min;
    const [maxX, maxY, maxZ] = blueprint.model.bounds.max;
    const size = new THREE.Vector3(maxX - minX, maxY - minY, maxZ - minZ);
    const dimensions = new THREE.Vector3(
      size.x > 0 ? size.x : 0.1,
      size.y > 0 ? size.y : 0.1,
      size.z > 0 ? size.z : 0.1
    );
    const minVector = new THREE.Vector3(minX, minY, minZ);
    const center = minVector.clone().add(size.clone().multiplyScalar(0.5));

    const boxGeometry = new THREE.BoxGeometry(
      dimensions.x,
      dimensions.y,
      dimensions.z
    );
    const edges = new THREE.EdgesGeometry(boxGeometry);
    boxGeometry.dispose();
    const material = new THREE.LineBasicMaterial({ color: 0x60a5fa });
    const box = new THREE.LineSegments(edges, material);
    box.position.set(center.x, center.y, center.z);
    box.visible = showBounds;

    scene.add(box);
    boundsRef.current = box;

    const camera = cameraRef.current;
    const controls = controlsRef.current;
    if (camera && controls) {
      const distance = dimensions.length() + 4;
      const offset = Math.max(distance, 10);
      controls.target.set(center.x, center.y, center.z);
      camera.position.set(center.x + offset, center.y + offset, center.z + offset);
      camera.near = Math.max(0.1, dimensions.length() * 0.05);
      camera.far = Math.max(500, dimensions.length() * 10 + 10);
      camera.updateProjectionMatrix();
      controls.update();
    }

    needsRenderRef.current = true;

    return () => {
      if (boundsRef.current) {
        scene.remove(boundsRef.current);
        boundsRef.current.geometry.dispose();
        const mat = boundsRef.current.material;
        if (Array.isArray(mat)) {
          mat.forEach((m) => m.dispose());
        } else {
          mat.dispose();
        }
        boundsRef.current = null;
        needsRenderRef.current = true;
      }
    };
  }, [blueprint]);

  useEffect(() => {
    const scene = sceneRef.current;
    if (!scene) {
      return;
    }

    if (meshRef.current) {
      scene.remove(meshRef.current);
      meshRef.current.geometry.dispose();
      const material = meshRef.current.material;
      if (Array.isArray(material)) {
        material.forEach((m) => m.dispose());
      } else {
        material.dispose();
      }
      meshRef.current = null;
      needsRenderRef.current = true;
    }

    if (!activeLod || activeLod.mesh.positions.length === 0) {
      return;
    }

    const geometry = new THREE.BufferGeometry();
    const positionAttribute = new THREE.BufferAttribute(activeLod.mesh.positions, 3);
    positionAttribute.setUsage(THREE.StaticDrawUsage);
    geometry.setAttribute("position", positionAttribute);

    const normalAttribute = new THREE.BufferAttribute(activeLod.mesh.normals, 3);
    normalAttribute.setUsage(THREE.StaticDrawUsage);
    geometry.setAttribute("normal", normalAttribute);

    const colorAttribute = new THREE.BufferAttribute(activeLod.mesh.colors, 3);
    colorAttribute.setUsage(THREE.StaticDrawUsage);
    geometry.setAttribute("color", colorAttribute);

    if (activeLod.mesh.indices.length > 0) {
      const indexAttribute = new THREE.BufferAttribute(activeLod.mesh.indices, 1);
      indexAttribute.setUsage(THREE.StaticDrawUsage);
      geometry.setIndex(indexAttribute);
    }

    geometry.computeBoundingBox();
    geometry.computeBoundingSphere();

    const material = new THREE.MeshBasicMaterial({
      vertexColors: true,
      toneMapped: false
    });

    const mesh = new THREE.Mesh(geometry, material);
    meshRef.current = mesh;
    scene.add(mesh);
    needsRenderRef.current = true;
  }, [activeLod]);

  useEffect(() => {
    if (boundsRef.current) {
      boundsRef.current.visible = showBounds;
    }
    needsRenderRef.current = true;
  }, [showBounds]);

  return <div ref={containerRef} className="scene-container" />;
};

export default SceneCanvas;
