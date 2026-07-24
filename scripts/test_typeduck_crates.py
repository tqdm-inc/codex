import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock


MODULE_PATH = Path(__file__).with_name("typeduck_crates.py")
SPEC = importlib.util.spec_from_file_location("typeduck_crates", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class TypeduckCratesTest(unittest.TestCase):
    def test_namespaces_support_packages_but_preserves_root_name(self):
        self.assertEqual(
            MODULE.published_name({"name": "codex-core"}),
            "typeduck-codex-core",
        )
        self.assertEqual(
            MODULE.published_name({"name": MODULE.ROOT_PACKAGE}),
            MODULE.ROOT_PACKAGE,
        )
        self.assertEqual(
            MODULE.registry_name({"name": "codex-core"}),
            MODULE.SUPPORT_REGISTRY_PACKAGE,
        )
        self.assertEqual(
            MODULE.registry_name({"name": MODULE.ROOT_PACKAGE}),
            MODULE.ROOT_PACKAGE,
        )
        self.assertEqual(
            MODULE.registry_name({"name": "codex-app-server-protocol"}),
            MODULE.SUPPORT_REGISTRY_OVERRIDES["codex-app-server-protocol"][0],
        )

    def test_previous_package_must_use_the_current_registry_name(self):
        manifest = {
            "packages": {
                "typeduck-codex-core": {
                    "registryName": MODULE.SUPPORT_REGISTRY_PACKAGE,
                    "version": "0.1.0",
                }
            }
        }

        self.assertIsNone(
            MODULE.compatible_previous_package(
                manifest,
                "typeduck-codex-core",
                "typeduck-cdx-dep-ae1ee23c4f7ae1e5",
            )
        )

    def test_version_reuses_identical_content_and_bumps_changes(self):
        previous = {"digest": "same", "version": "0.1.4"}
        self.assertEqual(
            MODULE.next_version(previous, "same", False),
            "0.1.4",
        )
        self.assertEqual(
            MODULE.next_version(previous, "changed", False),
            "0.1.5",
        )
        self.assertEqual(
            MODULE.next_version(previous, "changed", True),
            MODULE.ROOT_VERSION,
        )

    def test_support_packages_receive_distinct_minor_version_lines(self):
        mapped = {
            "async": {
                "name": MODULE.SUPPORT_REGISTRY_PACKAGE,
                "package": {"name": "codex-async-utils"},
                "registryName": MODULE.SUPPORT_REGISTRY_PACKAGE,
                "version": "0.1.0",
            },
            "other": {
                "name": "typeduck-codex-other",
                "package": {"name": "codex-other"},
                "registryName": None,
                "version": None,
            },
            "root": {
                "name": MODULE.ROOT_PACKAGE,
                "package": {"name": MODULE.ROOT_PACKAGE},
                "registryName": MODULE.ROOT_PACKAGE,
                "version": None,
            },
        }
        release_manifest = {
            "packages": {
                MODULE.SUPPORT_REGISTRY_PACKAGE: {
                    "registryName": MODULE.SUPPORT_REGISTRY_PACKAGE,
                    "version": "0.1.0",
                }
            }
        }

        MODULE.assign_initial_versions(mapped, release_manifest)

        self.assertEqual(
            {
                key: (entry["registryName"], entry["version"])
                for key, entry in mapped.items()
            },
            {
                "async": (MODULE.SUPPORT_REGISTRY_PACKAGE, "0.1.0"),
                "other": (MODULE.SUPPORT_REGISTRY_PACKAGES[1], "0.1.0"),
                "root": (MODULE.ROOT_PACKAGE, MODULE.ROOT_VERSION),
            },
        )

    def test_support_shards_balance_active_packages_not_tombstones(self):
        mapped = {
            "active": {
                "name": "typeduck-active",
                "package": {"name": "active"},
                "registryName": MODULE.SUPPORT_REGISTRY_PACKAGES[0],
                "version": "0.1.0",
            },
            "new": {
                "name": "typeduck-new",
                "package": {"name": "new"},
                "registryName": None,
                "version": None,
            },
            **{
                f"active-{index}": {
                    "name": f"typeduck-active-{index}",
                    "package": {"name": f"active-{index}"},
                    "registryName": registry,
                    "version": "0.1.0",
                }
                for index, registry in enumerate(
                    MODULE.SUPPORT_REGISTRY_PACKAGES[2:],
                    start=2,
                )
            },
        }
        tombstone_registry = MODULE.SUPPORT_REGISTRY_PACKAGES[1]
        release_manifest = {
            "packages": {
                "active": {
                    "registryName": MODULE.SUPPORT_REGISTRY_PACKAGES[0],
                    "version": "0.1.0",
                },
                **{
                    f"tombstone-{minor}": {
                        "registryName": tombstone_registry,
                        "version": f"0.{minor}.0",
                    }
                    for minor in range(1, 5)
                },
            }
        }

        MODULE.assign_initial_versions(mapped, release_manifest)

        self.assertEqual(
            (mapped["new"]["registryName"], mapped["new"]["version"]),
            (tombstone_registry, "0.5.0"),
        )

    def test_dependency_matching_falls_back_to_package_name(self):
        package_id = "git+tungstenite"
        utf8_id = "registry+utf-8"
        metadata = {
            "packages": [
                {
                    "id": package_id,
                    "name": "tungstenite",
                    "dependencies": [
                        {
                            "name": "utf-8",
                            "rename": None,
                            "kind": None,
                            "target": None,
                        }
                    ],
                },
                {"id": utf8_id, "name": "utf-8"},
            ],
            "resolve": {
                "nodes": [
                    {
                        "id": package_id,
                        "deps": [
                            {
                                "name": "utf8",
                                "pkg": utf8_id,
                                "dep_kinds": [{"kind": None, "target": None}],
                            }
                        ],
                    },
                    {"id": utf8_id, "deps": []},
                ]
            },
        }

        rows = MODULE.active_dependency_rows(package_id, metadata)

        self.assertEqual(rows[0][0], "utf-8")
        self.assertEqual(rows[0][2], utf8_id)

    def test_wildcard_dependency_uses_resolved_version_for_publication(self):
        metadata = {
            "packages": [
                {"id": "registry+openssl-sys", "version": "0.9.111"},
            ]
        }

        self.assertEqual(
            MODULE.published_dependency_requirement(
                {"req": "*"}, "registry+openssl-sys", metadata
            ),
            "=0.9.111",
        )

    def test_root_resolution_pins_keep_the_rama_alpha_family_coherent(self):
        rows = MODULE.root_resolution_pin_rows()

        self.assertEqual(len(rows), len(MODULE.ROOT_RESOLUTION_PINS))
        self.assertIn(
            '"rama-error" = { version = "=0.3.0-alpha.4", default-features = false }',
            rows,
        )

    def test_release_digest_changes_when_mapped_dependency_version_changes(self):
        source = str(MODULE_PATH)
        dependency_id = "dependency"
        parent_id = "parent"
        metadata = {
            "packages": [
                {
                    "id": dependency_id,
                    "name": "dependency",
                    "edition": "2024",
                    "manifest_path": source,
                    "dependencies": [],
                    "features": {},
                    "targets": [
                        {"name": "dependency", "kind": ["lib"], "src_path": source}
                    ],
                },
                {
                    "id": parent_id,
                    "name": "parent",
                    "edition": "2024",
                    "manifest_path": source,
                    "dependencies": [
                        {
                            "name": "dependency",
                            "rename": None,
                            "kind": None,
                            "target": None,
                            "req": "*",
                            "uses_default_features": True,
                            "features": [],
                            "optional": False,
                        }
                    ],
                    "features": {},
                    "targets": [
                        {"name": "parent", "kind": ["lib"], "src_path": source}
                    ],
                },
            ],
            "resolve": {
                "nodes": [
                    {"id": dependency_id, "deps": [], "features": []},
                    {
                        "id": parent_id,
                        "deps": [
                            {
                                "name": "dependency",
                                "pkg": dependency_id,
                                "dep_kinds": [{"kind": None, "target": None}],
                            }
                        ],
                        "features": [],
                    },
                ]
            },
        }
        mapped = {
            dependency_id: {
                "package": metadata["packages"][0],
                "name": "typeduck-dependency",
                "registryName": MODULE.SUPPORT_REGISTRY_PACKAGE,
                "version": "0.1.0",
                "sourceDigest": "dependency-source",
            },
            parent_id: {
                "package": metadata["packages"][1],
                "name": "typeduck-parent",
                "registryName": MODULE.SUPPORT_REGISTRY_PACKAGE,
                "version": "0.1.0",
                "sourceDigest": "parent-source",
            },
        }

        initial = MODULE.release_digest(parent_id, metadata, mapped)
        mapped[dependency_id]["version"] = "0.1.1"

        self.assertNotEqual(initial, MODULE.release_digest(parent_id, metadata, mapped))

        mapped[dependency_id]["version"] = "0.1.0"
        metadata["packages"][1]["edition"] = "2021"
        self.assertNotEqual(initial, MODULE.release_digest(parent_id, metadata, mapped))

    def test_unpublished_manifest_preserves_published_baseline(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "release-manifest.json"
            baseline = {"typeduck-support": {"digest": "old", "version": "0.1.4"}}
            path.write_text(
                json.dumps(
                    {
                        "schemaVersion": MODULE.MANIFEST_SCHEMA_VERSION,
                        "published": False,
                        "baselinePackages": baseline,
                        "packages": {
                            "typeduck-support": {
                                "digest": "new",
                                "version": "0.1.5",
                            }
                        },
                    }
                )
            )

            self.assertEqual(
                MODULE.load_release_manifest(path),
                {"packages": baseline},
            )

    def test_crates_io_rate_limit_uses_server_reset_time(self):
        output = (
            "status 429: Please try again after "
            "Wed, 22 Jul 2026 10:47:11 GMT and see the rate-limit documentation"
        )
        reset_at = 1784717231

        self.assertEqual(
            MODULE.crates_io_rate_limit_delay(output, reset_at - 30),
            32,
        )
        self.assertIsNone(MODULE.crates_io_rate_limit_delay("status 500", reset_at))

    def test_publish_resume_verifies_an_existing_exact_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "staged"
            output.mkdir()
            manifest = {
                "rootVersion": MODULE.ROOT_VERSION,
                "publishOrder": [MODULE.ROOT_PACKAGE],
                "packages": {MODULE.ROOT_PACKAGE: {"version": MODULE.ROOT_VERSION}},
            }
            manifest_path = output / "typeduck-crates-manifest.json"
            manifest_path.write_text(json.dumps(manifest))
            manifest_digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
            state_path = output.parent / (
                f"typeduck-crates-publish-state-{manifest_digest[:16]}.json"
            )
            state_path.write_text(
                json.dumps(
                    {
                        "manifestDigest": manifest_digest,
                        "completed": {},
                        "pending": {
                            f"{MODULE.ROOT_PACKAGE}@{MODULE.ROOT_VERSION}": "matching-checksum"
                        },
                    }
                )
            )

            with (
                mock.patch.object(
                    MODULE, "local_crate_checksum", return_value="matching-checksum"
                ),
                mock.patch.object(
                    MODULE, "crate_version_checksum", return_value="matching-checksum"
                ),
            ):
                release_manifest_path = output.parent / "release-manifest.json"
                MODULE.publish_all(output, manifest, True, release_manifest_path)

            state = json.loads(state_path.read_text())
            self.assertEqual(
                state["completed"],
                {f"{MODULE.ROOT_PACKAGE}@{MODULE.ROOT_VERSION}": "matching-checksum"},
            )
            self.assertEqual(state["pending"], {})
            self.assertTrue(json.loads(release_manifest_path.read_text())["published"])

    def test_publish_resume_adopts_an_uncheckpointed_exact_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "staged"
            output.mkdir()
            support = "typeduck-support"
            package_key = f"{support}@0.2.0"
            manifest = {
                "rootVersion": MODULE.ROOT_VERSION,
                "publishOrder": [support],
                "packages": {
                    support: {
                        "version": "0.2.0",
                        "registryName": support,
                    }
                },
            }
            manifest_path = output / "typeduck-crates-manifest.json"
            manifest_path.write_text(json.dumps(manifest))
            manifest_digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
            state_path = output.parent / (
                f"typeduck-crates-publish-state-{manifest_digest[:16]}.json"
            )
            state_path.write_text(
                json.dumps(
                    {
                        "manifestDigest": manifest_digest,
                        "completed": {},
                        "pending": {},
                    }
                )
            )

            with (
                mock.patch.object(
                    MODULE, "local_crate_checksum", return_value="matching-checksum"
                ) as local_checksum,
                mock.patch.object(
                    MODULE,
                    "crate_version_checksum",
                    side_effect=lambda name, _version: (
                        "matching-checksum" if name == support else None
                    ),
                ),
            ):
                release_manifest_path = output.parent / "release-manifest.json"
                MODULE.publish_all(output, manifest, True, release_manifest_path)

            local_checksum.assert_called_once_with(output, support, support, "0.2.0")
            state = json.loads(state_path.read_text())
            self.assertEqual(state["completed"], {package_key: "matching-checksum"})
            self.assertEqual(state["pending"], {})

    def test_publish_resume_rejects_an_existing_different_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "staged"
            output.mkdir()
            manifest = {
                "rootVersion": MODULE.ROOT_VERSION,
                "publishOrder": [MODULE.ROOT_PACKAGE],
                "packages": {MODULE.ROOT_PACKAGE: {"version": MODULE.ROOT_VERSION}},
            }
            manifest_path = output / "typeduck-crates-manifest.json"
            manifest_path.write_text(json.dumps(manifest))
            manifest_digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
            state_path = output.parent / (
                f"typeduck-crates-publish-state-{manifest_digest[:16]}.json"
            )
            state_path.write_text(
                json.dumps(
                    {
                        "manifestDigest": manifest_digest,
                        "completed": {},
                        "pending": {
                            f"{MODULE.ROOT_PACKAGE}@{MODULE.ROOT_VERSION}": "local"
                        },
                    }
                )
            )

            with (
                mock.patch.object(MODULE, "local_crate_checksum", return_value="local"),
                mock.patch.object(
                    MODULE, "crate_version_checksum", return_value="remote"
                ),
                self.assertRaisesRegex(RuntimeError, "does not match"),
            ):
                MODULE.publish_all(
                    output,
                    manifest,
                    True,
                    output.parent / "release-manifest.json",
                )

    def test_publish_resume_never_overwrites_a_pending_checksum(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "staged"
            output.mkdir()
            package_key = f"{MODULE.ROOT_PACKAGE}@{MODULE.ROOT_VERSION}"
            manifest = {
                "rootVersion": MODULE.ROOT_VERSION,
                "publishOrder": [MODULE.ROOT_PACKAGE],
                "packages": {MODULE.ROOT_PACKAGE: {"version": MODULE.ROOT_VERSION}},
            }
            manifest_path = output / "typeduck-crates-manifest.json"
            manifest_path.write_text(json.dumps(manifest))
            manifest_digest = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
            state_path = output.parent / (
                f"typeduck-crates-publish-state-{manifest_digest[:16]}.json"
            )
            state_path.write_text(
                json.dumps(
                    {
                        "manifestDigest": manifest_digest,
                        "completed": {},
                        "pending": {package_key: "original"},
                    }
                )
            )

            with (
                mock.patch.object(
                    MODULE, "local_crate_checksum", return_value="regenerated"
                ),
                mock.patch.object(MODULE, "crate_version_checksum", return_value=None),
                self.assertRaisesRegex(RuntimeError, "pending upload"),
            ):
                MODULE.publish_all(
                    output,
                    manifest,
                    True,
                    output.parent / "release-manifest.json",
                )

            self.assertEqual(
                json.loads(state_path.read_text())["pending"],
                {package_key: "original"},
            )

    def test_later_release_skips_unchanged_published_support_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "staged"
            output.mkdir()
            support = "typeduck-support"
            manifest = {
                "baselinePackages": {
                    "typeduck-removed": {
                        "checksum": "removed-checksum",
                        "digest": "removed-digest",
                        "registryName": MODULE.SUPPORT_REGISTRY_PACKAGE,
                        "version": "0.9.0",
                    }
                },
                "rootVersion": "0.3.0",
                "publishOrder": [support],
                "packages": {
                    support: {
                        "version": "0.1.0",
                        "previouslyPublished": True,
                        "publishedChecksum": "matching-checksum",
                    }
                },
            }
            (output / "typeduck-crates-manifest.json").write_text(json.dumps(manifest))
            release_manifest_path = output.parent / "release-manifest.json"

            with (
                mock.patch.object(
                    MODULE, "local_crate_checksum", return_value="matching-checksum"
                ),
                mock.patch.object(
                    MODULE,
                    "crate_version_checksum",
                    side_effect=lambda name, _version: (
                        "matching-checksum" if name == support else None
                    ),
                ),
            ):
                MODULE.publish_all(output, manifest, True, release_manifest_path)

            published = json.loads(release_manifest_path.read_text())
            self.assertTrue(published["published"])
            self.assertEqual(
                published["packages"]["typeduck-removed"],
                manifest["baselinePackages"]["typeduck-removed"],
            )


if __name__ == "__main__":
    unittest.main()
