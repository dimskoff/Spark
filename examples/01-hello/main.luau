-- The smallest 3D game: a ground, a spinning cube, a sun. Run: spark examples/01-hello
local cube

function start()
    sun(vec3(-0.4, -1, -0.3), "white", 1.2)
    sky({ top = "#4a7bd0", bottom = "#cfe3ff" })
    spawn(Mesh.plane(20), "#5c8a4d")
    cube = spawn(Mesh.cube(), "orange", { position = vec3(0, 0.5, 0) })
    camera.position = vec3(3, 2.5, 4)
    camera:look_at(vec3(0, 0.5, 0))
end

function update(dt)
    cube:rotate_y(dt)
end