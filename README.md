# Beacon In A Box

A set of docker images to create a Twine randomness beacon. This is built with
the [twine-rs](https://github.com/twine-protocol/twine-rs) library using
version 2 of the twine specification.

## What does it do?

This collection of docker services publishes and stores randomness
pulses in the form of "tixels", the basic data block of the Twine
protocol. These tixels are hash chained together forming a "strand".

## What's in the box?

The contents are separated into a few docker services that are configured
using a `docker-compose.yaml` file. They are:

1. A generator which is the main process that schedules the construction
and release of pulse data in the form of twine tixels. The twine data
can be signed using either a locally stored secret key in pem format, or
a yubihsm2 device.
2. A mysql database. This uses the official mysql image.
3. A data-sync service: which synchronizes the locally stored twine data
with a remote twine HTTP server of one's choosing.
4. A local HTTP server which is intended to be an internal tool for viewing
the twine data.

## Prerequisites

1. [Docker](https://www.docker.com/)
2. (optional) [YubiHSM2 SDK](https://developers.yubico.com/YubiHSM2/Releases/) if
using a YubiHSM2 for signing. See "Configuring for YubiHSM2".

## Getting started

### Create a randomness source

The beacon is configured to run a specified command to get random bits.
This command must return 512 bits (64 bytes) in binary format to stdout.
The beacon specification requires at least two random sources be used
and their outputs be concatenated and hashed together.

The `python_example/get_randomness.py` script serves as a basic example
of how to do this. A production setup may require dependencies which
will need to be properly configured by editing `Dockerfile.base`.

The command to run is configured in the `docker-compose.yaml` file
with the `RNG_SCRIPT` environment variable.

### Strand configuration files

Create a `.config/` directory

```sh
mkdir .config
```

Generate an RSA private key (skip this if using HSM).

```sh
bash keygen.sh .config
```

Create a `.config/strand-config.json` file specifying the metadata to include in the
strand definition. All of this information is vendor specific and intended
to describe the strand, its owner, and scope.

Example:

```json
{
  "details": {
    "name": "ACME randomness strand",
    "website": "https://acme.com",
    "desciption": "512 bits of public randomness released every minute."
  }
}
```

Create a `.config/stitch-map.yaml` which controls what strand data to pull and
stitch. This only affects the local strand. Upon creation the strand CID
should be shared with the owners of the external strands to enable them
to stitch your strand in kind.

NOTE: This file can be edited "in flight" while the beacon is running
and changes will get applied as of the next pulse.

Example:

```yaml
stitches:
  - resolver: https://some-twine-http-service.dev
    strand: bafyrmieej3j3sprtnbfziv6vhixzr3xxrcabnma43ajb5grhsixdvxzdvu
    stop: false # if set to true, the stitch updating with be paused
```

### Generator lead time configuration

The environment variable `LEAD_TIME_SECONDS` defines the number of seconds
in advance that the generator should prepare the next pulse. Adjust this time
to give ample time to obtain randomness, construct the pulse, and sign it.

### External store synchronization

The `data_sync` service configured in the `docker-compose.yaml` file is a service
that will synchronize local data with a remote server.

The following variables should be customized:

- `REMOTE_STORE_ADDRESS`: the url of the remote store
- `REMOTE_STORE_API_KEY`: an optional api key
- `SYNC_PERIOD_SECONDS`: The inverval at which data is re-checked.

Note: The sync service will be notified of changes when a new pulse is constructed
and will immediately sync the changes, however the service also checks
sync state on a regular interval as well, for redundancy.

### Database

The database will automatically setup itself upon boot using the
`sql/mysql-schema.sql` file. For more information about configuring
the docker mysql image, see the [docker mysql documenation](https://hub.docker.com/_/mysql/).

### Starting the services

Initial startup will result in the strand being created which will output
a `strand.json` file into the `.config/` directory. The generator
will immediately queue construction of the next pulse for the
top of the approaching minute.

To build and run the docker services, issue the following command:

```sh
docker compose up --build -d
```

## Configuring for YubiHSM2

The yubihsm-connector service is used to connect to the HSM. Install this from the
[YubiHSM2 SKD](https://developers.yubico.com/YubiHSM2/Releases/). Initial
provisioning of the HSM is covered by the [YubiHSM2 documentation](https://docs.yubico.com/hardware/yubihsm-2/hsm-2-user-guide/hsm2-introduction.html).

Note: The yubihsm-connector service must be configured to run constantly in order to
communicate with the hsm. The way this is setup depends on the host system OS. It is possible to run this in a docker container, however, it is extremely cumbersome
to do in OSX, due to docker being unable to communicate with USB devices without
a virtual machine.

The device should be setup with an authentication key with permissions `sign-rsa`,
along with an assymetric key with type `rsa2048`.

The following environment variables must be set in a `.env` file.

- `HSM_ADDRESS`: the domain and port for the yubihsm-connecter service (likely: `host.docker.internal:12345`)
- `HSM_AUTH_KEY_ID`: the authentication key id
- `HSM_PASSWORD`: the authentication key password
- `HSM_SIGNING_KEY_ID`: the (rsa) signing key id

This .env file should have restrictive permissions (`0600`) to prevent unauthorized
access.

The docker-compose.yaml file then needs to be edited to use this file for the
generator service:

```yaml
services:
  generator:
    env_file:
      - .env
```

