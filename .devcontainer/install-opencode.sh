#!/usr/bin/env bash
# ============================================================================
# install-ai-tools.sh
# -------------------
# Instala APM (Agent Package Manager), OpenCode, y la extensión gem-team
# para OpenCode desde un devcontainer fresh.
#
# Flujo:
#   1. APM via installer oficial (curl -sSL https://aka.ms/apm-unix | sh)
#      → si falla por incompatibilidad de glibc, fallback a pipx
#   2. OpenCode via installer oficial (curl -fsSL https://opencode.ai/install | bash)
#   3. gem-team via apm install mubaidr/gem-team --target opencode
#
# Maneja:
#   - Incompatibilidad de glibc (Debian 12 bookworm → glibc 2.36 vs 2.38 requerido)
#   - PEP 668 (entorno Python externamente gestionado, pip bloqueado)
#   - Idempotencia (seguro de re-ejecutar)
# ============================================================================
set -euo pipefail

# ── Colores ────────────────────────────────────────────────────────────────
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
ok()   { echo -e "  ${GREEN}✔${NC} $1"; }
warn() { echo -e "  ${YELLOW}⚠${NC} $1" >&2; }
fail() { echo -e "  ${RED}✖${NC} $1" >&2; }

# ── Preflight ──────────────────────────────────────────────────────────────
preflight() {
  for cmd in curl sudo; do
    if ! command -v "$cmd" &>/dev/null; then
      fail "Requerido: $cmd no está instalado"
      return 1
    fi
  done
}

# ── Asegurar directorios comunes en PATH ─────────────────────────────────────
# pipx → ~/.local/bin, opencode → ~/.opencode/bin, etc.
# Algunos entornos (devcontainer) no los traen por defecto en PATH.
ensure_common_bins_in_path() {
  local dirs
  dirs=(
    "$HOME/.local/bin"
    "$HOME/.opencode/bin"
  )
  for dir in "${dirs[@]}"; do
    if [[ -d "$dir" ]] && [[ ":$PATH:" != *":$dir:"* ]]; then
      export PATH="$dir:$PATH"
    fi
  done
}

# ── 1. APM ─────────────────────────────────────────────────────────────────
install_apm() {
  if command -v apm &>/dev/null; then
    ok "APM ya instalado ($(apm --version 2>&1))"
    return 0
  fi

  echo ""
  echo "  ── Instalando APM (Agent Package Manager) ──"

  # Intento 1 — installer oficial
  # Funciona en sistemas con glibc >= 2.38.
  if curl -sSL https://aka.ms/apm-unix | sh; then
    if command -v apm &>/dev/null; then
      ok "APM instalado via installer oficial"
      return 0
    fi
  fi

  # Intento 2 — pipx
  # Fallback para Debian 12 (bookworm) y distribuciones similares donde
  # el binario precompilado requiere glibc 2.38 pero el sistema tiene 2.36.
  warn "Installer oficial falló — usando pipx como fallback..."
  if ! command -v pipx &>/dev/null; then
    sudo apt-get update -qq
    sudo apt-get install -y --no-install-recommends pipx
  fi

  pipx install apm-cli
  ensure_common_bins_in_path

  if command -v apm &>/dev/null; then
    ok "APM instalado via pipx ($(apm --version 2>&1))"
  else
    fail "No se pudo instalar APM."
    fail "Intenta manualmente: pipx install apm-cli"
    return 1
  fi
}

# ── 2. OpenCode ────────────────────────────────────────────────────────────
install_opencode() {
  if command -v opencode &>/dev/null; then
    ok "OpenCode ya instalado ($(opencode --version 2>&1))"
    return 0
  fi

  echo ""
  echo "  ── Instalando OpenCode ──"

  # CI=true suprime la barra de progreso del installer (usa \r)
  if CI=true curl -fsSL https://opencode.ai/install | bash; then
    # opencode se instala en ~/.opencode/bin/
    ensure_common_bins_in_path
    if command -v opencode &>/dev/null; then
      ok "OpenCode instalado ($(opencode --version 2>&1))"
    else
      fail "OpenCode instalado pero no encontrado en PATH."
      fail "Prueba reiniciar tu shell o agregar ~/.opencode/bin a PATH."
      return 1
    fi
  else
    fail "Error al instalar OpenCode."
    fail "Intenta manualmente: curl -fsSL https://opencode.ai/install | bash"
    return 1
  fi
}

# ── 3. gem-team ────────────────────────────────────────────────────────────
install_gem_team() {
  if ! command -v apm &>/dev/null; then
    fail "APM no está instalado — no se puede instalar gem-team."
    return 1
  fi

  echo ""
  echo "  ── Instalando extensión gem-team para OpenCode ──"
  echo "  Comando: apm install mubaidr/gem-team --target opencode"
  echo ""

  if apm install mubaidr/gem-team --target opencode; then
    ok "Extensión gem-team instalada correctamente"
  else
    # Puede fallar si ya está instalada o si opencode no es detectable
    warn "No se pudo instalar gem-team."
    warn "Verifica con: apm list --target opencode"
  fi
}

# ── Resumen final ──────────────────────────────────────────────────────────
print_summary() {
  echo ""
  echo "  ────────────────────────────────────────────────"
  ok "Instalación completada"
  echo ""

  # Verificar estado final de cada herramienta
  local all_ok=true

  if command -v apm &>/dev/null; then
    ok "APM:        $(apm --version 2>&1)"
  else
    fail "APM:        NO INSTALADO"
    all_ok=false
  fi

  if command -v opencode &>/dev/null; then
    ok "OpenCode:   $(opencode --version 2>&1)"
  else
    fail "OpenCode:   NO INSTALADO"
    all_ok=false
  fi

  # gem-team: verificar via apm.yml (dónde apm escribe las dependencias)
  if [[ -f "$PWD/apm.yml" ]] && grep -q 'mubaidr/gem-team' "$PWD/apm.yml" 2>/dev/null; then
    ok "gem-team:   Instalada en apm.yml"
  else
    warn "gem-team:   No detectada en apm.yml"
  fi

  echo ""
  if ! $all_ok; then
    warn "Algunas herramientas no se instalaron correctamente."
    warn "Revisa los mensajes de error arriba o ejecuta el script de nuevo."
  fi

  # Sugerir reinicio de shell
  echo ""
  warn "Si algún comando recién instalado no se encuentra en el PATH, prueba:"
  warn "  source ~/.zshrc  (o reinicia la terminal)"
}

# ── Main ───────────────────────────────────────────────────────────────────
main() {
  echo ""
  echo "  ┌──────────────────────────────────────────────┐"
  echo "  │        Instalación de herramientas AI        │"
  echo "  └──────────────────────────────────────────────┘"

  preflight || { fail "Preflight falló"; return 1; }

  install_apm
  install_opencode
  install_gem_team

  print_summary
}

main "$@"