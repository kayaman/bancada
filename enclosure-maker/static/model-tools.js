class ModelTools {
  constructor({ scene, camera, renderer, orbit, onCommit }) {
    Object.assign(this, { scene, camera, renderer, orbit, onCommit });
    this.root = new THREE.Group();
    scene.add(this.root);
    this.parts = [];
    this.objects = new Map();
    this.history = [];
    this.future = [];
    this.pending = null;
    this.fitted = false;
    this.selectEl = document.getElementById('part-select');
    this.feedback = document.getElementById('object-feedback');
    this.fields = ['move-x', 'move-y', 'move-z', 'rotate-x', 'rotate-y', 'rotate-z'].map(id => document.getElementById(id));
    this.gizmo = renderer && THREE.TransformControls ? new THREE.TransformControls(camera, renderer.domElement) : null;
    if (this.gizmo) {
      scene.add(this.gizmo);
      this.gizmo.setSpace('world');
      this.gizmo.addEventListener('dragging-changed', event => { if (orbit) orbit.enabled = !event.value; });
      this.gizmo.addEventListener('objectChange', () => this.refreshFields());
      this.gizmo.addEventListener('mouseUp', () => this.commit(this.readObject()));
      this.gizmo.addEventListener('mouseDown', () => { this.feedback.textContent = 'Drag an axis. Release to save.'; });
    }
    this.selectEl.addEventListener('change', () => this.select(this.selectEl.value));
    this.fields.forEach(field => field.addEventListener('change', () => {
      if (!this.fields.every(input => input.value !== '' && Number.isFinite(input.valueAsNumber) && Math.abs(input.valueAsNumber) <= 100000)) {
        this.feedback.textContent = 'Enter valid position and rotation numbers.';
        this.refreshFields();
        return;
      }
      this.commit({ translation: this.fields.slice(0, 3).map(f => f.valueAsNumber), rotation: this.fields.slice(3).map(f => f.valueAsNumber) });
    }));
    document.getElementById('move-tool').addEventListener('click', () => this.mode('translate'));
    document.getElementById('rotate-tool').addEventListener('click', () => this.mode('rotate'));
    document.getElementById('fit-model').addEventListener('click', () => this.fit());
    document.querySelectorAll('[data-view]').forEach(button => button.addEventListener('click', () => this.fit(button.dataset.view)));
    document.getElementById('snap-transform').addEventListener('change', event => {
      if (!this.gizmo) return;
      this.gizmo.setTranslationSnap(event.target.checked ? 1 : null);
      this.gizmo.setRotationSnap(event.target.checked ? Math.PI / 12 : null);
    });
    document.getElementById('reset-transform').addEventListener('click', () => this.commit({ translation: [0, 0, 0], rotation: [0, 0, 0] }));
    document.getElementById('undo-transform').addEventListener('click', () => this.undo());
    document.getElementById('redo-transform').addEventListener('click', () => this.redo());
    window.addEventListener('keydown', event => {
      if (event.target.closest('input, textarea, select, [contenteditable]')) return;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z') {
        event.preventDefault(); event.shiftKey ? this.redo() : this.undo();
      } else if (event.key.toLowerCase() === 'w') this.mode('translate');
      else if (event.key.toLowerCase() === 'e') this.mode('rotate');
      else if (event.key.toLowerCase() === 'f') this.fit();
    });
    if (renderer) {
      let down = null;
      renderer.domElement.addEventListener('pointerdown', event => {
        down = { x: event.clientX, y: event.clientY, handle: !!this.gizmo?.axis };
      });
      renderer.domElement.addEventListener('pointerup', event => {
        if (!down || down.handle || this.pending || Math.hypot(event.clientX - down.x, event.clientY - down.y) > 5) return;
        const rect = renderer.domElement.getBoundingClientRect();
        const ray = new THREE.Raycaster();
        ray.setFromCamera(new THREE.Vector2((event.clientX - rect.left) / rect.width * 2 - 1, -(event.clientY - rect.top) / rect.height * 2 + 1), camera);
        const hit = ray.intersectObjects(this.root.children, true)[0];
        if (hit) this.select(hit.object.parent.name);
      });
    }
    this.refreshFields();
  }

  setGeometry(buffer) { this.buffer = buffer; }

  setParts(parts) {
    if (!this.buffer || !Array.isArray(parts)) return;
    const count = new DataView(this.buffer).getUint32(0, true);
    if (parts.some(p => p.start_triangle < 0 || p.triangle_count < 0 || p.start_triangle + p.triangle_count > count)) return;
    const selected = this.selected;
    this.gizmo?.detach();
    for (const object of this.objects.values()) {
      object.children[0].geometry.dispose();
      object.children[0].material.dispose();
      this.root.remove(object);
    }
    this.objects.clear();
    this.parts = parts;
    this.selectEl.replaceChildren();
    for (const part of parts) {
      const option = document.createElement('option'); option.value = part.name; option.textContent = part.name;
      this.selectEl.appendChild(option);
      const positions = new Float32Array(this.buffer, 4 + part.start_triangle * 36, part.triangle_count * 9).slice();
      for (let i = 0; i < positions.length; i++) positions[i] -= part.center[i % 3];
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
      geometry.computeVertexNormals(); geometry.computeBoundingSphere();
      const material = new THREE.MeshStandardMaterial({ color: 0x5b8dee, metalness: 0.1, roughness: 0.6, side: THREE.DoubleSide });
      const object = new THREE.Group(); object.name = part.name;
      object.add(new THREE.Mesh(geometry, material));
      this.objects.set(part.name, object); this.root.add(object);
      this.applyObject(part.name, part);
    }
    this.selectEl.disabled = parts.length === 0 || !!this.pending;
    this.select(parts.some(p => p.name === selected) ? selected : parts[0]?.name);
    if (parts.some(p => p.triangle_count > 0) && !this.fitted) { this.fit(); this.fitted = true; }
  }

  select(name) {
    this.selected = name;
    if (name) this.selectEl.value = name;
    for (const [key, object] of this.objects) object.children[0].material.color.setHex(key === name ? 0x85b2ff : 0x5b8dee);
    const object = this.objects.get(name);
    if (object && !this.pending) this.gizmo?.attach(object); else this.gizmo?.detach();
    this.refreshFields();
  }

  readObject() {
    const object = this.objects.get(this.selected);
    const part = this.parts.find(p => p.name === this.selected);
    if (!object || !part) return null;
    return {
      translation: object.position.toArray().map((v, i) => v - part.center[i]),
      rotation: [object.rotation.x, object.rotation.y, object.rotation.z].map(THREE.MathUtils.radToDeg),
    };
  }

  applyObject(name, transform) {
    const object = this.objects.get(name);
    const part = this.parts.find(p => p.name === name);
    if (!object || !part) return;
    object.position.set(...part.center.map((v, i) => v + transform.translation[i]));
    object.rotation.set(...transform.rotation.map(THREE.MathUtils.degToRad), 'ZYX');
    object.updateMatrixWorld(true);
  }

  refreshFields() {
    const transform = this.readObject();
    this.fields.forEach((field, i) => {
      field.disabled = !transform || !!this.pending;
      if (document.activeElement !== field) field.value = transform ? Number([...transform.translation, ...transform.rotation][i].toFixed(6)) : 0;
    });
    document.getElementById('reset-transform').disabled = !transform || !!this.pending;
    document.getElementById('undo-transform').disabled = !this.history.length || !!this.pending;
    document.getElementById('redo-transform').disabled = !this.future.length || !!this.pending;
    this.selectEl.disabled = !this.parts.length || !!this.pending;
  }

  commit(transform, action = 'edit', entry = null) {
    if (!transform || this.pending || !this.selected) return;
    const part = this.parts.find(p => p.name === this.selected);
    const before = { translation: [...part.translation], rotation: [...part.rotation] };
    if (JSON.stringify(before) === JSON.stringify(transform)) return;
    const id = `${Date.now()}-${Math.random()}`;
    this.pending = { id, name: this.selected, before, after: transform, action, entry };
    this.applyObject(this.selected, transform);
    this.feedback.textContent = 'Saving…';
    this.gizmo?.detach(); this.refreshFields();
    try { this.onCommit(this.selected, transform, id); }
    catch (error) { this.result({ request_id: id, ok: false, message: error.message }); }
  }

  result(message) {
    if (!this.pending || message.request_id !== this.pending.id) return;
    const pending = this.pending; this.pending = null;
    if (message.ok) {
      if (pending.action === 'edit') { this.history.push(pending); this.future = []; }
      else if (pending.action === 'undo') { this.history.pop(); this.future.push(pending.entry); }
      else { this.future.pop(); this.history.push(pending.entry); }
      const part = this.parts.find(p => p.name === pending.name);
      if (part) Object.assign(part, pending.after);
      this.feedback.textContent = 'Saved · included in exports';
    } else {
      this.applyObject(pending.name, pending.before);
      this.feedback.textContent = message.message || 'Could not save this change.';
    }
    this.select(this.selected);
  }

  disconnect() {
    if (this.pending) this.result({ request_id: this.pending.id, ok: false, message: 'Connection lost. Reconnect to check the saved position.' });
  }

  undo() {
    const entry = this.history[this.history.length - 1];
    if (!entry || this.pending || !this.objects.has(entry.name)) return;
    this.select(entry.name); this.commit(entry.before, 'undo', entry);
  }

  redo() {
    const entry = this.future[this.future.length - 1];
    if (!entry || this.pending || !this.objects.has(entry.name)) return;
    this.select(entry.name); this.commit(entry.after, 'redo', entry);
  }

  mode(mode) {
    this.gizmo?.setMode(mode);
    document.getElementById('move-tool').setAttribute('aria-pressed', String(mode === 'translate'));
    document.getElementById('rotate-tool').setAttribute('aria-pressed', String(mode === 'rotate'));
  }

  fit(view = 'iso') {
    if (!this.root.children.length) return;
    const box = new THREE.Box3().setFromObject(this.root);
    if (box.isEmpty()) return;
    const center = box.getCenter(new THREE.Vector3());
    const radius = box.getSize(new THREE.Vector3()).length() / 2 || 10;
    // Reserve space for the inspector so the object stays beside it.
    const usable = Math.max(0.25, 1 - 310 / (this.renderer?.domElement.clientWidth || 1200));
    const distance = radius / Math.sin(THREE.MathUtils.degToRad(this.camera.fov / 2)) / Math.min(1, this.camera.aspect * usable) * 1.15;
    const direction = { iso: [1, -1, 0.8], top: [0, 0.001, 1], front: [0, -1, 0], right: [1, 0, 0] }[view];
    this.camera.up.set(...(view === 'top' ? [0, 1, 0] : [0, 0, 1]));
    this.camera.position.copy(center).add(new THREE.Vector3(...direction).normalize().multiplyScalar(distance));
    this.camera.near = Math.max(0.01, distance / 10000); this.camera.far = Math.max(10000, distance * 100);
    this.camera.updateProjectionMatrix(); this.camera.lookAt(center);
    if (this.orbit) { this.orbit.target.copy(center); this.orbit.update(); }
  }
}
