# container-store
This is a utility for NixOS. It is easy to run applications in isolation on NixOS.
However, when applications use the nix store of the root filesystem, they have
access to the whole system configuration.

To limit this access, `container-store` creates an overlay over `/nix/store/`
and removes any paths which are not needed. Additionally, the access rights are
set to execute only `--x`, which means applications cannot list the contents.

## Usage
By default, `container-store` creates overlays in `/var/lib/container-stores`.
This can be changed by passing `--root <path>`.

The arguments are paths pointing into the nix store. These paths and their
dependencies will be preserved in the overlay while the rest is deleted.
The dependencies are computed by `nix-store -qR <paths>`.

License
-------
Licensed under either of

 * [Apache License, Version 2.0](LICENSE-APACHE)
 * [MIT license](LICENSE-MIT)

at your option.
