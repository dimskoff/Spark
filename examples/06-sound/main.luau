-- Sound: Space plays a one-shot, M toggles looping music-like loop, a 3D sound pans as the cube moves.
local ball, loop, t = nil, nil, 0

function start()
    sun(vec3(-0.4, -1, -0.3), "white", 1.1)
    spawn(Mesh.plane(20), "#444c5c")
    ball = spawn(Mesh.sphere(0.4), "#ff7755", { position = vec3(-4, 0.5, 0) })
    camera.position = vec3(0, 3, 8)
    camera:look_at(vec3(0, 0.5, 0))
end

function update(dt)
    t += dt
    ball.x = math.sin(t) * 4
    if input.pressed("space") then sound.play("sounds/hit.wav", { volume = 0.8 }) end
    if input.pressed("m") then
        if loop then loop:stop(0.2) loop = nil else loop = sound.play("sounds/spawn.wav", { loop = true, at = ball, range = 14 }) end
    end
end

function render()
    draw.text("Space: hit   M: 3D loop on the moving ball", 16, 16, { size = 22, shadow = true })
end