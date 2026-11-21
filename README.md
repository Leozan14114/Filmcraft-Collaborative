# Filmcraft-Collaborative
An unofficial fork of FilmCraft adding real-time collaborative editing. Work on the same edit simultaneously with others without passing files back and forth.

**FilmCraft Collaborative** is an unofficial fork of [FilmCraft](https://github.com/storytold/filmcraft) (originally developed by ArtCraft) that adds **real-time collaborative editing**, allowing multiple users to work on the same edit simultaneously without manually exporting and sharing project files.

> **Disclaimer:** This is an independent community modification and is not affiliated with, endorsed, or officially supported by ArtCraft.

## Key Features
* **Simultaneous Collaborative Editing:** Multiple users can edit the same project at the same time.
* **No Manual File Sharing:** Real-time synchronization of changes without passing files back and forth.
* Includes all original features of FilmCraft.

## License

This project is a derivative work based on the original source code of [FilmCraft](https://github.com/storytold/filmcraft) by **ArtCraft**.

In compliance with the original project's distribution terms, the entire codebase of **FilmCraft Collaborative** (including all modifications and newly added features) is licensed under the same dual license:

* **MIT License** (see [LICENSE-MIT](LICENSE-MIT))
* **Apache License, Version 2.0** (see [LICENSE-APACHE](LICENSE-APACHE))

Anyone wishing to use, modify, or redistribute this code is free to do so under the terms of either license above, provided that the original copyright notices from **ArtCraft**, the copyright notices for **FilmCraft Collaborative**, and the respective license files are kept intact.

#### Quick Start: Collaborative Relay Server (Docker)

To collaborate in real time, one user (or a dedicated server) needs to host the relay instance using Docker. 

#### Prerequisites

* [Docker](https://docs.docker.com/get-docker/) and **Docker Compose** installed on the host machine.

#### 1. Start the Relay Server

Clone the repository and launch the container in detached mode: 


git clone https://github.com/Leozan14114/Filmcraft-Collaborative.git
cd Filmcraft-Collaborative
docker compose -f compose.relay.yaml up -d --build

#### 2. Verify It's Running

Check the container status and logs to confirm the server is listening: 

docker compose -f compose.relay.yaml ps
docker compose -f compose.relay.yaml logs -f

#### 3. Connect from FilmCraft

1. Make sure the relay port (configured in compose.relay.yaml) is reachable on your local network or forwarded on your router/firewall for remote collaborators.
2. Open **FilmCraft Collaborative** on each client machine and enter the host's IP address (or domain) and port to join the same session.

#### Stop the Server

docker compose -f compose.relay.yaml down
