-- Showcase: a grid of PBR spheres (metallic 0..1 left to right, roughness 0..1 front to back) under a sky with bloom and ACES.
function start()
    sky({ top = "#3b6fb8", bottom = "#e9dcc5", lighting = 1.0 })
    sun(vec3(-0.5, -0.9, -0.4), "#fff3dd", 2.2)
    graphics.tonemap = "aces"
    graphics.bloom = { threshold = 1.0, strength = 0.25 }
    graphics.ssao = true
    graphics.fxaa = true
    spawn(Mesh.plane(40), Material.pbr("#3a3d45", 0.0, 0.6))
    for row = 0, 4 do
        for col = 0, 4 do
            local m = Material.pbr("#d8a45a", col / 4, math.max(0.05, row / 4))
            spawn(Mesh.sphere(0.45, 48), m, { position = vec3((col - 2) * 1.2, 0.5, (row - 2) * 1.2) })
        end
    end
    camera.position = vec3(0, 5.2, 7.5)
    camera:look_at(vec3(0, 0.3, 0))
    camera.fov = 45
end

function update(dt)
    local a = time.elapsed * 0.15
    camera.position = vec3(math.sin(a) * 8, 5.2, math.cos(a) * 8)
    camera:look_at(vec3(0, 0.3, 0))
end