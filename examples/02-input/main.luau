-- Keyboard, mouse and gamepad: WASD / left stick moves the cube, Space jumps height, click spawns a marker.
local player
local y_vel = 0

function start()
    sun(vec3(-0.4, -1, -0.3), "white", 1.1)
    spawn(Mesh.plane(30), "#4d6b5a")
    player = spawn(Mesh.cube(), "#ff8844", { position = vec3(0, 0.5, 0) })
    camera.position = vec3(0, 9, 9)
    camera:look_at(vec3(0, 0, 0))
end

function update(dt)
    local x = (input.down("d") and 1 or 0) - (input.down("a") and 1 or 0) + input.pad_axis("left_x")
    local z = (input.down("s") and 1 or 0) - (input.down("w") and 1 or 0) + input.pad_axis("left_y")
    player:translate(vec3(x, 0, z) * 6 * dt)
    if input.pressed("space") and player.y <= 0.5 then y_vel = 6 end
    y_vel = y_vel - 18 * dt
    player.y = math.max(0.5, player.y + y_vel * dt)
    if input.mouse_pressed("left") then
        spawn(Mesh.sphere(0.2), "yellow", { position = player.position + vec3(0, 1, 0) })
    end
end

function render()
    draw.text("WASD / stick: move   Space: jump   Click: marker", 16, 16, { size = 22, shadow = true })
end