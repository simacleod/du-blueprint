export interface BoundsResponse {
  min: [number, number, number];
  max: [number, number, number];
}

export interface ModelResponse {
  name: string;
  size: number;
  bounds: BoundsResponse;
}

export interface LodResponse {
  level: number;
  voxelSize: number;
  voxelCount: number;
  mesh: MeshResponse;
}

export interface MeshResponse {
  positions: string;
  normals: string;
  colors: string;
  indices: string;
}

export interface BlueprintResponse {
  model: ModelResponse;
  lods: LodResponse[];
}

export interface MeshBuffers {
  positions: Float32Array;
  normals: Float32Array;
  colors: Float32Array;
  indices: Uint32Array;
}

export interface LodData {
  level: number;
  voxelSize: number;
  voxelCount: number;
  mesh: MeshBuffers;
}

export interface BlueprintData {
  model: ModelResponse;
  lods: LodData[];
}

export const decodeBlueprint = (response: BlueprintResponse): BlueprintData => ({
  model: response.model,
  lods: response.lods.map((lod) => ({
    level: lod.level,
    voxelSize: lod.voxelSize,
    voxelCount: lod.voxelCount,
    mesh: decodeMesh(lod.mesh)
  }))
});

const decodeMesh = (mesh: MeshResponse): MeshBuffers => ({
  positions: decodeFloat32(mesh.positions),
  normals: decodeFloat32(mesh.normals),
  colors: decodeFloat32(mesh.colors),
  indices: decodeUint32(mesh.indices)
});

const decodeFloat32 = (encoded: string): Float32Array => {
  if (!encoded) {
    return new Float32Array();
  }
  const buffer = decodeBase64(encoded);
  return new Float32Array(buffer);
};

const decodeUint32 = (encoded: string): Uint32Array => {
  if (!encoded) {
    return new Uint32Array();
  }
  const buffer = decodeBase64(encoded);
  return new Uint32Array(buffer);
};

const decodeBase64 = (encoded: string): ArrayBuffer => {
  const normalized = encoded.length % 4 === 0 ? encoded : encoded.padEnd(encoded.length + (4 - (encoded.length % 4)), "=");
  const binary = atob(normalized);
  const length = binary.length;
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes.buffer;
};
