# Déploiement de handy-server

Serveur autonome de transcription pour Handy, sans dépendance GTK/Tauri.

## Prérequis

- Linux (testé sur Arch)
- Rust + Cargo (pour build local) ou binaire pré-packagé
- GPU Vulkan (optionnel, fallback CPU)
- Tailscale (recommandé pour accès distant sécurisé)

## Build & packaging

```bash
cd handy-server
./scripts/package.sh --release
```

Cela produit `dist/` avec :
- `handy-server` — binaire avec rpath `$ORIGIN/lib`
- `lib/*.so` — bibliothèques natives (transcribe-cpp, ggml backends)

**Pas besoin de `LD_LIBRARY_PATH` ni `DISPLAY`.**

## Installation via systemd

```bash
sudo ./packaging/install.sh
```

L'installateur :
1. Copie `dist/` → `/opt/handy-server/`
2. Crée `/etc/handy-server/env` (chmod 600) avec token généré
3. Installe l'unit systemd et démarre le service

### Configuration

Fichier `/etc/handy-server/env` :

```ini
HANDY_TOKEN=mon_token_secret
HANDY_MODEL=handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf
HANDY_BIND=0.0.0.0:8756
# HANDY_LANGUAGE=auto
# HANDY_DEVICE=vulkan
# HANDY_GPU_DEVICE=0
```

Après modification : `sudo systemctl restart handy-server`

### Gestion du service

```bash
systemctl status handy-server
journalctl -u handy-server -f
sudo systemctl restart handy-server
```

## Ports

- **8756** par défaut (pas 8080 — souvent pris par `llama-server`)
- Le `/health` de `handy-server` inclut `"engine":"handy"` pour le distinguer d'autres services

## Test depuis le client Windows

Dans l'UI Handy → onglet Serveur :
- URL : `http://<ip-tailscale-mini-pc>:8756`
- Token : valeur de `HANDY_TOKEN` dans `/etc/handy-server/env`

Le test de connexion vérifie maintenant `engine == "handy"` (T4.1).

## Commandes CLI utiles

```bash
# Lister les devices GPU
/opt/handy-server/handy-server devices

# Lister les modèles installés
/opt/handy-server/handy-server models list

# Télécharger un modèle
/opt/handy-server/handy-server models download handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf

# Selftest sur un modèle
/opt/handy-server/handy-server selftest --model handy-computer/canary-1b-v2-gguf/canary-1b-v2-Q5_K_M.gguf

# Générer un token
/opt/handy-server/handy-server token generate
```

## Dépannage

### "Ce port répond mais n'est pas un serveur Handy"
Un autre service écoute sur le port (ex. `llama-server` sur 8080). Vérifiez :
```bash
ss -tlnp | grep 8756
curl http://localhost:8756/health | jq .engine  # doit retourner "handy"
```

### Erreur de chargement de modèle
Vérifier les logs : `journalctl -u handy-server -n 50`
Le modèle peut être absent — utiliser `models download` ou attendre le téléchargement automatique via `POST /models/load`.

### VRAM partagée avec llama-server
Sur le mini-PC avec 2 GPU AMD, utiliser `HANDY_GPU_DEVICE=1` pour isoler handy-server sur un GPU dédié. Vérifier avec `handy-server devices`.

### Modèles ONNX refusés
Les modèles ONNX sont gated (D10) tant que le bug F1 n'est pas résolu. Passer `--allow-onnx` ou utiliser un modèle GGUF.

## Structure des dossiers

```
/opt/handy-server/
├── handy-server      # binaire
└── lib/              # .so natives

/etc/handy-server/
└── env               # variables d'environnement (chmod 600)

~/.local/share/com.pais.handy/models/
├── handy-computer/   # modèles GGUF
└── onnx/             # modèles ONNX
```

## Migration depuis --serve Tauri

L'ancien mode `--serve` reste fonctionnel mais nécessite un display GTK. Pour migrer :
1. Installer `handy-server` comme documenté ci-dessus
2. Changer l'URL dans les settings du client Windows
3. Le protocole HTTP est identique — le client fonctionne sans modification

**Phase 5 recommandée** : une fois validé, retirer `serve.rs` et la branche `--serve` du fork pour réduire les conflits upstream.
