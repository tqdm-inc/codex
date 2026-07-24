#!/usr/bin/env python3
"""Stage and publish the standalone typeduck-codex-web Cargo package graph."""

import argparse
from email.utils import parsedate_to_datetime
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import quote as url_quote
from urllib.request import Request
from urllib.request import urlopen


ROOT_PACKAGE = "typeduck-codex-web"
ROOT_VERSION = "0.6.0"
SUPPORT_VERSION = "0.1.0"
SUPPORT_REGISTRY_PACKAGES = [
    "typeduck-codex-async-utils",
    "typeduck-codex-utils-absolute-path",
    "typeduck-codex-execpolicy",
    "typeduck-codex-extension-items",
    "typeduck-codex-utils-home-dir",
    "typeduck-codex-utils-rustls-provider",
]
SUPPORT_REGISTRY_PACKAGE = SUPPORT_REGISTRY_PACKAGES[0]
SUPPORT_REGISTRY_OVERRIDES = {
    "codex-app-server-protocol": ("typeduck-cdx-dep-1624b46487e59efe", "0.2.0"),
}
ROOT_RESOLUTION_PINS = [
    "rama-core",
    "rama-dns",
    "rama-error",
    "rama-http",
    "rama-http-backend",
    "rama-http-core",
    "rama-http-headers",
    "rama-http-types",
    "rama-macros",
    "rama-net",
    "rama-socks5",
    "rama-tcp",
    "rama-tls-rustls",
    "rama-udp",
    "rama-unix",
    "rama-utils",
]
RAMA_VERSION = "0.3.0-alpha.4"
MANIFEST_SCHEMA_VERSION = 2
RATE_LIMIT_RESET_PATTERN = re.compile(r"try again after (.+? GMT) and see")
IGNORED_NAMES = {
    ".git",
    ".jj",
    "target",
    "Cargo.lock",
    "Cargo.toml",
}


def run(command, *, cwd, capture=False, environment=None):
    result = subprocess.run(
        command,
        cwd=cwd,
        check=True,
        text=True,
        env=environment,
        stdout=subprocess.PIPE if capture else None,
    )
    return result.stdout if capture else ""


def cargo_metadata(workspace):
    encoded = run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=workspace,
        capture=True,
    )
    return json.loads(encoded)


def package_map(metadata):
    return {package["id"]: package for package in metadata["packages"]}


def node_map(metadata):
    return {node["id"]: node for node in metadata["resolve"]["nodes"]}


def resolved_closure(metadata):
    packages = package_map(metadata)
    root_id = next(
        package_id
        for package_id, package in packages.items()
        if package["name"] == ROOT_PACKAGE
    )
    seen = set()
    pending = [root_id]
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        pending.extend(
            dependency_id
            for _alias, _dependency, dependency_id in active_dependency_rows(
                package_id, metadata
            )
        )
    return seen


def is_mapped(package, workspace_members):
    source = package.get("source")
    return package["id"] in workspace_members or (
        isinstance(source, str) and source.startswith("git+")
    )


def published_name(package):
    if package["name"] == ROOT_PACKAGE:
        return ROOT_PACKAGE
    return "typeduck-" + package["name"]


def registry_name(package):
    if package["name"] == ROOT_PACKAGE:
        return ROOT_PACKAGE
    override = SUPPORT_REGISTRY_OVERRIDES.get(package["name"])
    return override[0] if override else SUPPORT_REGISTRY_PACKAGES[0]


def compatible_previous_package(release_manifest, name, registry):
    previous = release_manifest["packages"].get(name)
    if previous is None or previous.get("registryName") != registry:
        return None
    return previous


def source_root(package):
    return Path(package["manifest_path"]).resolve().parent


def source_digest(package):
    root = source_root(package)
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if not path.is_file() or any(part in IGNORED_NAMES for part in path.parts):
            continue
        relative = path.relative_to(root).as_posix()
        digest.update(relative.encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def package_description(package):
    if package["name"] == ROOT_PACKAGE:
        return "A standalone browser interface and Codex runtime"
    return package.get("description") or (
        f"Support package for the standalone Codex Web runtime ({package['name']})"
    )


def root_resolution_pin_rows():
    return [
        f"{quote(name)} = {{ version = {quote('=' + RAMA_VERSION)}, default-features = false }}"
        for name in ROOT_RESOLUTION_PINS
    ]


def load_release_manifest(path):
    if not path.exists():
        return {"packages": {}}
    manifest = json.loads(path.read_text())
    if manifest.get("schemaVersion") != MANIFEST_SCHEMA_VERSION:
        return {"packages": {}}
    if not manifest.get("published", False):
        return {"packages": manifest.get("baselinePackages", {})}
    return manifest


def next_version(previous, digest, is_root, initial_version=SUPPORT_VERSION):
    if is_root:
        return ROOT_VERSION
    if previous is None:
        return initial_version
    if previous["digest"] == digest:
        return previous["version"]
    major, minor, patch = map(int, previous["version"].split("."))
    return f"{major}.{minor}.{patch + 1}"


def generated_package_metadata(entry, release_manifest):
    previous = (
        compatible_previous_package(
            release_manifest, entry["name"], entry["registryName"]
        )
        or {}
    )
    previously_published = (
        previous.get("version") == entry["version"]
        and previous.get("digest") == entry["digest"]
    )
    return {
        "digest": entry["digest"],
        "version": entry["version"],
        "registryName": entry["registryName"],
        "source": entry["package"].get("source") or "workspace",
        "previouslyPublished": previously_published,
        "publishedChecksum": previous.get("checksum") if previously_published else None,
    }


def mapped_packages(metadata, release_manifest):
    packages = package_map(metadata)
    closure = resolved_closure(metadata)
    workspace_members = set(metadata["workspace_members"])
    mapped = {}
    for package_id in closure:
        package = packages[package_id]
        if not is_mapped(package, workspace_members):
            continue
        name = published_name(package)
        existing = release_manifest["packages"].get(name)
        override = SUPPORT_REGISTRY_OVERRIDES.get(package["name"])
        expected_registry = (
            override[0]
            if override
            else existing.get("registryName")
            if existing
            else registry_name(package)
            if package["name"] == ROOT_PACKAGE
            else None
        )
        previous = compatible_previous_package(
            release_manifest, name, expected_registry
        )
        mapped[package_id] = {
            "package": package,
            "name": name,
            "registryName": expected_registry,
            "version": (
                previous["version"] if previous else override[1] if override else None
            ),
            "sourceDigest": source_digest(package),
        }
    names = [entry["name"] for entry in mapped.values()]
    if len(names) != len(set(names)):
        raise RuntimeError("mapped package names are not unique")
    assign_initial_versions(mapped, release_manifest)
    return mapped


def assign_initial_versions(mapped, release_manifest):
    used_minors = {registry: set() for registry in SUPPORT_REGISTRY_PACKAGES}
    for package in release_manifest["packages"].values():
        registry = package.get("registryName")
        if registry in used_minors:
            used_minors[registry].add(int(package["version"].split(".")[1]))
    active_counts = {registry: 0 for registry in SUPPORT_REGISTRY_PACKAGES}
    for entry in mapped.values():
        registry = entry["registryName"]
        if registry in active_counts:
            active_counts[registry] += 1
    entries = sorted(
        mapped.values(),
        key=lambda entry: (
            entry["name"] != SUPPORT_REGISTRY_PACKAGE,
            entry["name"],
        ),
    )
    for entry in entries:
        if entry["package"]["name"] == ROOT_PACKAGE:
            entry["version"] = ROOT_VERSION
            continue
        if entry["version"] is not None:
            continue
        if (
            entry["registryName"] is not None
            and entry["registryName"] not in SUPPORT_REGISTRY_PACKAGES
        ):
            entry["version"] = SUPPORT_VERSION
            continue
        registry = min(
            SUPPORT_REGISTRY_PACKAGES,
            key=lambda candidate: (
                active_counts[candidate],
                len(used_minors[candidate]),
                max(used_minors[candidate], default=0),
                SUPPORT_REGISTRY_PACKAGES.index(candidate),
            ),
        )
        next_minor = 1
        while next_minor in used_minors[registry]:
            next_minor += 1
        entry["registryName"] = registry
        entry["version"] = f"0.{next_minor}.0"
        used_minors[registry].add(next_minor)
        active_counts[registry] += 1


def release_digest(package_id, metadata, mapped):
    package = mapped[package_id]["package"]
    dependencies = []
    aliases = set()
    for alias, dependency, dependency_id in active_dependency_rows(
        package_id, metadata
    ):
        aliases.add(alias)
        mapped_dependency = mapped.get(dependency_id)
        dependencies.append(
            {
                "alias": alias,
                "package": (
                    mapped_dependency["registryName"]
                    if mapped_dependency
                    else dependency["name"]
                ),
                "version": (
                    "=" + mapped_dependency["version"]
                    if mapped_dependency
                    else dependency["req"]
                ),
                "kind": dependency.get("kind"),
                "target": dependency.get("target"),
                "defaultFeatures": dependency.get("uses_default_features", True),
                "features": sorted(dependency.get("features", [])),
                "optional": dependency.get("optional", False),
            }
        )
    targets = [
        {
            "name": target["name"],
            "kind": sorted(target["kind"]),
            "path": relative_source_path(package, target["src_path"]),
        }
        for target in package["targets"]
        if any(
            kind in {"bin", "lib", "proc-macro", "custom-build"}
            for kind in target["kind"]
        )
    ]
    semantics = {
        "source": mapped[package_id]["sourceDigest"],
        "package": {
            "description": package_description(package),
            "edition": package.get("edition"),
            "rustVersion": package.get("rust_version"),
            "license": package.get("license") or "Apache-2.0",
            "repository": package.get("repository")
            or "https://github.com/tqdm-inc/codex",
        },
        "dependencies": sorted(
            dependencies,
            key=lambda dependency: (
                dependency["alias"],
                dependency["kind"] or "",
                dependency["target"] or "",
            ),
        ),
        "features": filtered_features(package_id, metadata, aliases),
        "targets": sorted(targets, key=lambda target: (target["name"], target["path"])),
    }
    encoded = json.dumps(semantics, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def finalize_versions(ordered, metadata, mapped, release_manifest):
    for package_id in ordered:
        entry = mapped[package_id]
        digest = release_digest(package_id, metadata, mapped)
        previous = compatible_previous_package(
            release_manifest, entry["name"], entry["registryName"]
        )
        entry["digest"] = digest
        entry["version"] = next_version(
            previous,
            digest,
            entry["package"]["name"] == ROOT_PACKAGE,
            entry["version"],
        )


def topological_order(metadata, mapped):
    nodes = node_map(metadata)
    visited = set()
    visiting = set()
    ordered = []

    def visit(package_id):
        if package_id in visited:
            return
        if package_id in visiting:
            raise RuntimeError(f"dependency cycle at {package_id}")
        visiting.add(package_id)
        for _alias, _dependency, dependency_id in active_dependency_rows(
            package_id, metadata
        ):
            if dependency_id in mapped:
                visit(dependency_id)
        visiting.remove(package_id)
        visited.add(package_id)
        ordered.append(package_id)

    root_id = next(
        package_id
        for package_id, entry in mapped.items()
        if entry["package"]["name"] == ROOT_PACKAGE
    )
    visit(root_id)
    return ordered


def quote(value):
    return json.dumps(value, ensure_ascii=False)


def relative_source_path(package, absolute):
    return Path(absolute).resolve().relative_to(source_root(package)).as_posix()


def active_dependency_rows(package_id, metadata):
    packages = package_map(metadata)
    nodes = node_map(metadata)
    package = packages[package_id]
    node = nodes[package_id]
    active_edges = {}
    for edge in node["deps"]:
        for kind in edge["dep_kinds"]:
            key = (
                edge["name"],
                kind.get("kind"),
                kind.get("target"),
            )
            active_edges[key] = edge["pkg"]

    rows = []
    for dependency in package["dependencies"]:
        if dependency.get("kind") == "dev":
            continue
        alias = dependency.get("rename") or dependency["name"]
        key = (
            alias.replace("-", "_"),
            dependency.get("kind"),
            dependency.get("target"),
        )
        package_id_for_dependency = active_edges.get(key)
        if package_id_for_dependency is None:
            matches = {
                edge_package_id
                for (
                    edge_name,
                    edge_kind,
                    edge_target,
                ), edge_package_id in active_edges.items()
                if edge_kind == dependency.get("kind")
                and edge_target == dependency.get("target")
                and packages[edge_package_id]["name"] == dependency["name"]
            }
            if len(matches) == 1:
                package_id_for_dependency = matches.pop()
        if package_id_for_dependency is None:
            continue
        rows.append((alias, dependency, package_id_for_dependency))
    return rows


def published_dependency_requirement(dependency, dependency_id, metadata):
    requirement = dependency["req"]
    if requirement != "*":
        return requirement
    return f"={package_map(metadata)[dependency_id]['version']}"


def dependency_tables(package_id, metadata, mapped):
    tables = {}
    aliases = set()
    for alias, dependency, dependency_id in active_dependency_rows(
        package_id, metadata
    ):
        kind = dependency.get("kind")
        target = dependency.get("target")
        if kind == "build":
            table = "build-dependencies"
        else:
            table = "dependencies"
        key = (target, table)
        aliases.add(alias)
        if dependency_id in mapped:
            mapped_dependency = mapped[dependency_id]
            fields = [
                f"package = {quote(mapped_dependency['registryName'])}",
                f"version = {quote('=' + mapped_dependency['version'])}",
            ]
        else:
            requirement = published_dependency_requirement(
                dependency, dependency_id, metadata
            )
            fields = [f"version = {quote(requirement)}"]
        if not dependency.get("uses_default_features", True):
            fields.append("default-features = false")
        if dependency.get("features"):
            features = ", ".join(quote(feature) for feature in dependency["features"])
            fields.append(f"features = [{features}]")
        if dependency.get("optional"):
            fields.append("optional = true")
        tables.setdefault(key, []).append(
            f"{quote(alias)} = {{ " + ", ".join(fields) + " }"
        )
    return tables, aliases


def filtered_features(package_id, metadata, aliases):
    packages = package_map(metadata)
    nodes = node_map(metadata)
    package = packages[package_id]
    active = set(nodes[package_id]["features"])
    features = {}
    for name, members in package.get("features", {}).items():
        if name not in active and name != "default":
            continue
        kept = []
        for member in members:
            dependency_name = member.removeprefix("dep:").split("/", 1)[0].rstrip("?")
            if member.startswith("dep:") or "/" in member:
                if dependency_name not in aliases:
                    continue
            kept.append(member)
        features[name] = kept
    return features


def license_file(package, repository_root):
    package_root = source_root(package)
    candidates = [
        package_root / "LICENSE",
        package_root / "LICENSE-APACHE",
        repository_root / "LICENSE",
    ]
    current = package_root
    for _ in range(5):
        candidates.extend([current / "LICENSE", current / "LICENSE-APACHE"])
        current = current.parent
    return next((candidate for candidate in candidates if candidate.is_file()), None)


def copy_package(entry, destination, repository_root):
    package = entry["package"]
    source = source_root(package)
    shutil.copytree(
        source,
        destination,
        ignore=shutil.ignore_patterns(*IGNORED_NAMES),
    )
    existing_license = any(
        child.name.startswith("LICENSE") for child in destination.iterdir()
    )
    if not existing_license:
        license_path = license_file(package, repository_root)
        if license_path is None:
            raise RuntimeError(f"no license file for {package['name']}")
        shutil.copy2(license_path, destination / license_path.name)


def render_manifest(package_id, metadata, mapped, repository_root):
    entry = mapped[package_id]
    package = entry["package"]
    lines = [
        "[package]",
        f"name = {quote(entry['registryName'])}",
        f"version = {quote(entry['version'])}",
        f"edition = {quote(package['edition'])}",
        f"description = {quote(package_description(package))}",
        f"license = {quote(package.get('license') or 'Apache-2.0')}",
        f"repository = {quote(package.get('repository') or 'https://github.com/tqdm-inc/codex')}",
        "autobins = false",
        "autoexamples = false",
        "autotests = false",
        "autobenches = false",
    ]
    if package.get("rust_version"):
        lines.append(f"rust-version = {quote(package['rust_version'])}")
    if package["name"] == ROOT_PACKAGE:
        lines.extend(
            [
                f"readme = {quote('README.md')}",
                f"keywords = [{quote('codex')}, {quote('cli')}, {quote('web')}, {quote('ai')}]",
                f"categories = [{quote('command-line-utilities')}, {quote('development-tools')}]",
            ]
        )

    targets = package["targets"]
    build_target = next(
        (target for target in targets if "custom-build" in target["kind"]), None
    )
    if build_target:
        lines.append(
            f"build = {quote(relative_source_path(package, build_target['src_path']))}"
        )

    library = next(
        (
            target
            for target in targets
            if "lib" in target["kind"] or "proc-macro" in target["kind"]
        ),
        None,
    )
    if library:
        lines.extend(
            [
                "",
                "[lib]",
                f"name = {quote(library['name'])}",
                f"path = {quote(relative_source_path(package, library['src_path']))}",
                "doctest = false",
            ]
        )
        if "proc-macro" in library["kind"]:
            lines.append("proc-macro = true")

    if package["name"] == ROOT_PACKAGE:
        binary = next(target for target in targets if "bin" in target["kind"])
        lines.extend(
            [
                "",
                "[[bin]]",
                f"name = {quote(ROOT_PACKAGE)}",
                f"path = {quote(relative_source_path(package, binary['src_path']))}",
            ]
        )

    tables, aliases = dependency_tables(package_id, metadata, mapped)
    for (target, table), rows in sorted(
        tables.items(), key=lambda item: (item[0][0] or "", item[0][1])
    ):
        lines.append("")
        if target:
            lines.append(f"[target.{quote(target)}.{table}]")
        else:
            lines.append(f"[{table}]")
        rendered_rows = list(rows)
        if (
            package["name"] == ROOT_PACKAGE
            and target is None
            and table == "dependencies"
        ):
            rendered_rows.extend(root_resolution_pin_rows())
        lines.extend(sorted(rendered_rows))

    features = filtered_features(package_id, metadata, aliases)
    if features:
        lines.extend(["", "[features]"])
        for name, members in sorted(features.items()):
            encoded = ", ".join(quote(member) for member in members)
            lines.append(f"{quote(name)} = [{encoded}]")
    lines.append("")
    return "\n".join(lines)


def stage(workspace, output, release_manifest_path, update_manifest):
    metadata = cargo_metadata(workspace)
    repository_root = workspace.parent
    release_manifest = load_release_manifest(release_manifest_path)
    mapped = mapped_packages(metadata, release_manifest)
    ordered = topological_order(metadata, mapped)
    finalize_versions(ordered, metadata, mapped, release_manifest)

    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    shutil.copy2(workspace / "rust-toolchain.toml", output / "rust-toolchain.toml")
    shutil.copy2(workspace / "Cargo.lock", output / "Cargo.lock")

    for package_id in ordered:
        entry = mapped[package_id]
        destination = output / entry["name"]
        copy_package(entry, destination, repository_root)
        manifest = render_manifest(package_id, metadata, mapped, repository_root)
        (destination / "Cargo.toml").write_text(manifest)

    root_entry = next(
        entry for entry in mapped.values() if entry["package"]["name"] == ROOT_PACKAGE
    )
    support_entries = [
        mapped[package_id]
        for package_id in ordered
        if mapped[package_id]["package"]["name"] != ROOT_PACKAGE
    ]
    workspace_manifest = [
        "[workspace]",
        'resolver = "2"',
        f"members = [{quote(root_entry['name'])}]",
        "exclude = [",
    ]
    workspace_manifest.extend(f"  {quote(entry['name'])}," for entry in support_entries)
    workspace_manifest.extend(["]", "", "[patch.crates-io]"])
    for entry in support_entries:
        workspace_manifest.append(
            f"{quote(entry['name'])} = {{ package = {quote(entry['registryName'])}, path = {quote(entry['name'])} }}"
        )
    (output / "Cargo.toml").write_text("\n".join(workspace_manifest) + "\n")

    generated_manifest = {
        "schemaVersion": MANIFEST_SCHEMA_VERSION,
        "published": False,
        "baselinePackages": release_manifest["packages"],
        "rootVersion": ROOT_VERSION,
        "publishOrder": [mapped[package_id]["name"] for package_id in ordered],
        "packages": {
            entry["name"]: generated_package_metadata(entry, release_manifest)
            for entry in mapped.values()
        },
    }
    (output / "typeduck-crates-manifest.json").write_text(
        json.dumps(generated_manifest, indent=2, sort_keys=True) + "\n"
    )
    if update_manifest:
        write_json_atomic(release_manifest_path, generated_manifest)
    return generated_manifest


def verify(output):
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(output.parent / "typeduck-crates-target")
    run(
        ["cargo", "build", "--release", "-p", ROOT_PACKAGE],
        cwd=output,
        environment=environment,
    )


def package_all(output, manifest):
    package_dir = output / "packages"
    package_dir.mkdir(exist_ok=True)
    for name in manifest["publishOrder"]:
        run(
            [
                "cargo",
                "package",
                "--manifest-path",
                str(output / name / "Cargo.toml"),
                "--allow-dirty",
                "--no-verify",
                "--target-dir",
                str(output.parent / "typeduck-crates-target"),
            ],
            cwd=output,
        )


def crates_io_rate_limit_delay(output, now):
    match = RATE_LIMIT_RESET_PATTERN.search(output)
    if match is None:
        return None
    reset_at = parsedate_to_datetime(match.group(1)).timestamp()
    return max(1, math.ceil(reset_at - now) + 2)


def publish_crate(command, cwd):
    result = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    if result.returncode == 0:
        return None
    combined_output = result.stdout + result.stderr
    delay = crates_io_rate_limit_delay(combined_output, time.time())
    if delay is not None:
        return delay
    raise subprocess.CalledProcessError(
        result.returncode,
        command,
        output=result.stdout,
        stderr=result.stderr,
    )


def publish_all(output, manifest, execute, release_manifest_path):
    if not execute:
        raise RuntimeError("publication requires --execute")
    manifest_digest = hashlib.sha256(
        (output / "typeduck-crates-manifest.json").read_bytes()
    ).hexdigest()
    state_path = output.parent / (
        f"typeduck-crates-publish-state-{manifest_digest[:16]}.json"
    )
    resumed = state_path.exists()
    if resumed:
        state = json.loads(state_path.read_text())
        if state.get("manifestDigest") != manifest_digest:
            raise RuntimeError(
                f"publish state at {state_path} belongs to a different staged graph"
            )
    else:
        state = {"manifestDigest": manifest_digest, "completed": {}, "pending": {}}
    completed = state["completed"]
    pending = state.setdefault("pending", {})
    if not isinstance(completed, dict):
        raise RuntimeError(f"publish state at {state_path} uses an obsolete format")
    if not isinstance(pending, dict):
        raise RuntimeError(
            f"publish state at {state_path} has invalid pending artifacts"
        )
    root_key = f"{ROOT_PACKAGE}@{manifest['rootVersion']}"
    if not resumed and crate_version_checksum(ROOT_PACKAGE, manifest["rootVersion"]):
        raise RuntimeError(
            f"{root_key} already exists; bump ROOT_VERSION before publishing this graph"
        )
    write_json_atomic(state_path, state)
    for name in manifest["publishOrder"]:
        version = manifest["packages"][name]["version"]
        registry_package = manifest["packages"][name].get("registryName", name)
        package_key = f"{registry_package}@{version}"
        if package_key in completed:
            remote_checksum = crate_version_checksum(registry_package, version)
            if remote_checksum != completed[package_key]:
                raise RuntimeError(
                    f"published artifact for {package_key} changed after it was recorded"
                )
            continue
        remote_checksum = crate_version_checksum(registry_package, version)
        if remote_checksum is not None:
            expected_checksum = pending.get(package_key) or manifest["packages"][
                name
            ].get("publishedChecksum")
            if expected_checksum is None:
                expected_checksum = local_crate_checksum(
                    output,
                    name,
                    registry_package,
                    version,
                )
            if remote_checksum != expected_checksum:
                raise RuntimeError(
                    f"published artifact for {package_key} does not match the recorded crate"
                )
            completed[package_key] = remote_checksum
            pending.pop(package_key, None)
            write_json_atomic(state_path, state)
            continue
        local_checksum = local_crate_checksum(
            output,
            name,
            registry_package,
            version,
        )
        if package_key in pending:
            if local_checksum != pending[package_key]:
                raise RuntimeError(
                    f"staged artifact for pending upload {package_key} changed"
                )
            local_checksum = pending[package_key]
        else:
            pending[package_key] = local_checksum
            write_json_atomic(state_path, state)
        command = [
            "cargo",
            "publish",
            "--manifest-path",
            str(output / name / "Cargo.toml"),
            "--allow-dirty",
            "--no-verify",
        ]
        attempt = 0
        while True:
            try:
                delay = publish_crate(command, output)
                if delay is not None:
                    print(f"crates.io rate limit reached; resuming in {delay} seconds")
                    time.sleep(delay)
                    continue
                break
            except subprocess.CalledProcessError:
                if attempt == 5:
                    raise
                attempt += 1
                time.sleep(10 * attempt)
        remote_checksum = crate_version_checksum(registry_package, version)
        if remote_checksum != local_checksum:
            raise RuntimeError(
                f"published artifact for {package_key} does not match the uploaded crate"
            )
        completed[package_key] = remote_checksum
        pending.pop(package_key, None)
        write_json_atomic(state_path, state)
    published_packages = dict(manifest.get("baselinePackages", {}))
    published_packages.update(
        {
            name: {
                **metadata,
                "checksum": completed[
                    f"{metadata.get('registryName', name)}@{metadata['version']}"
                ],
            }
            for name, metadata in manifest["packages"].items()
        }
    )
    published_manifest = dict(manifest)
    published_manifest["published"] = True
    published_manifest.pop("baselinePackages", None)
    published_manifest["packages"] = published_packages
    write_json_atomic(release_manifest_path, published_manifest)


def write_json_atomic(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as temporary:
            temporary_path = Path(temporary.name)
            json.dump(value, temporary, indent=2, sort_keys=True)
            temporary.write("\n")
            temporary.flush()
            os.fsync(temporary.fileno())
        os.replace(temporary_path, path)
        temporary_path = None
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def local_crate_checksum(output, directory_name, registry_package, version):
    target_dir = output.parent / "typeduck-crates-target"
    run(
        [
            "cargo",
            "package",
            "--manifest-path",
            str(output / directory_name / "Cargo.toml"),
            "--allow-dirty",
            "--no-verify",
            "--target-dir",
            str(target_dir),
        ],
        cwd=output,
    )
    archive = target_dir / "package" / f"{registry_package}-{version}.crate"
    if not archive.is_file():
        raise RuntimeError(
            f"cargo did not create the expected archive for {registry_package}@{version}"
        )
    return hashlib.sha256(archive.read_bytes()).hexdigest()


def crate_version_checksum(name, version):
    url = f"https://crates.io/api/v1/crates/{url_quote(name)}/{url_quote(version)}"
    request = Request(url, headers={"User-Agent": "typeduck-codex-release/0.2"})
    try:
        with urlopen(request, timeout=30) as response:
            payload = json.load(response)
            return payload["version"]["checksum"]
    except HTTPError as error:
        if error.code == 404:
            return None
        raise RuntimeError(
            f"crates.io lookup failed for {name}@{version}: {error}"
        ) from error


def parse_args():
    repository_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=["stage", "verify", "package", "publish"],
    )
    parser.add_argument(
        "--workspace",
        type=Path,
        default=repository_root / "codex-rs",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=repository_root / "target" / "typeduck-crates",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=repository_root / "scripts" / "typeduck_crates_manifest.json",
    )
    parser.add_argument("--update-manifest", action="store_true")
    parser.add_argument("--execute", action="store_true")
    return parser.parse_args()


def main():
    args = parse_args()
    if args.command == "stage":
        manifest = stage(
            args.workspace.resolve(),
            args.output.resolve(),
            args.manifest.resolve(),
            args.update_manifest,
        )
        print(f"staged {len(manifest['publishOrder'])} packages in {args.output}")
        return
    manifest_path = args.output / "typeduck-crates-manifest.json"
    if not manifest_path.exists():
        raise RuntimeError("run the stage command first")
    manifest = json.loads(manifest_path.read_text())
    if args.command == "verify":
        verify(args.output.resolve())
    elif args.command == "package":
        package_all(args.output.resolve(), manifest)
    elif args.command == "publish":
        publish_all(
            args.output.resolve(),
            manifest,
            args.execute,
            args.manifest.resolve(),
        )


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
