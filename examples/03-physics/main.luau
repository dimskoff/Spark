-- Rigid bodies, collisions and raycasts: a stack of crates, balls to throw (click), a ray that finds what is under the cursor.
local hits = 0

function start()
    sun(vec3(-0.5, -1, -0.3), "white", 1.2)
    sky(true)
    spawn(Mesh.plane(40), "#556b55", { body = "static" })
    for i = 0, 5 do
        spawn(Mesh.box(1, 1, 1), "#c8873a", { position = vec3(0, 0.5 + i * 1.02, 0), body = "dynamic" })
    end
    camera.position = vec3(6, 5, 9)
    camera:look_at(vec3(0, 2, 0))
    on("collision", function() hits += 1 end)
end

function update(dt)
    if input.mouse_pressed("left") then
        local ball = spawn(Mesh.sphere(0.35), "#e04a4a", { position = camera.position + vec3(0, -0.5, -1), body = { shape = "sphere", radius = 0.35, bounciness = 0.3 } })
        ball.velocity = vec3(-0.6, 0.4, -1) * 22
    end
end

function render()
    draw.text("Click: throw a ball   collisions: " .. hits, 16, 16, { size = 22, shadow = true })
end