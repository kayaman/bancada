import type {
  CircuitCatalog,
  CircuitComponent,
  CircuitConnection,
  CircuitManifest,
  ComponentTemplate,
} from "./api";

export const fqbnTarget = (fqbn: string) => fqbn.split(":").slice(0, 3).join(":");

const nextId = (prefix: string, ids: readonly string[]) => {
  let suffix = 1;
  while (ids.includes(`${prefix}_${suffix}`)) suffix += 1;
  return `${prefix}_${suffix}`;
};

export function newCircuitManifest(
  catalog: CircuitCatalog,
  projectName: string,
  activeFqbn?: string,
): CircuitManifest {
  const board =
    catalog.boards.find(
      (candidate) =>
        activeFqbn && fqbnTarget(candidate.fqbn) === fqbnTarget(activeFqbn),
    ) ?? catalog.boards[0];
  return {
    version: catalog.version,
    name: `${projectName || "Project"} circuit`,
    board: { catalog_id: board.id, fqbn: board.fqbn },
    components: [],
    connections: [],
    notes: {},
  };
}

export function addComponent(
  manifest: CircuitManifest,
  template: ComponentTemplate,
): CircuitManifest {
  const id = nextId(template.kind, manifest.components.map((item) => item.id));
  const component: CircuitComponent = {
    id,
    label: template.label,
    kind: template.kind,
    verified: false,
    pins: template.pins.map((pin) => ({ id: pin, label: pin })),
    properties: {},
  };
  return { ...manifest, components: [...manifest.components, component] };
}

export function addConnection(manifest: CircuitManifest): CircuitManifest {
  const component = manifest.components[0];
  const id = nextId("wire", manifest.connections.map((item) => item.id));
  const connection: CircuitConnection = {
    id,
    from: { target: "board", pin: "GND" },
    to: {
      target: component?.id ?? "component",
      pin: component?.pins[0]?.id ?? "pin",
    },
    role: "ground",
  };
  return { ...manifest, connections: [...manifest.connections, connection] };
}

export function selectCircuitBoard(
  manifest: CircuitManifest,
  catalog: CircuitCatalog,
  boardId: string,
): CircuitManifest {
  const board = catalog.boards.find((item) => item.id === boardId);
  return board
    ? { ...manifest, board: { catalog_id: board.id, fqbn: board.fqbn } }
    : manifest;
}
