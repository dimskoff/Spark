-- Showcase: a night scene with many colored point lights, shadows, fog and bloom.
function start()
    sky({ top = "#050814", bottom = "#10162c", lighting = 0.15 })
    sun(vec3(-0.3, -1, -0.2), "#334466", 0.25)
    fog("#0a0e1c", 8, 40)
    graphics.tonemap = "aces"
    graphics.bloom = { threshold = 0.9, strength = 0.5 }
    graphics.light_shadows = 4
    spawn(Mesh.plane(40), Material.pbr("#2a2d33", 0.0, 0.35))
    local colors = { "#ff4a4a", "#4aff7a", "#4a8cff", "#ffd24a", "#ff4aff", "#4affff" }
    for i = 1, 12 do
        local a = i / 12 * math.pi * 2
        local pos = vec3(math.cos(a) * 5, 0.7, math.sin(a) * 5)
        spawn(Mesh.box(0.8, 1.4, 0.8), Material.pbr("#c9c9d0", 0.1, 0.5), { position = pos - vec3(0, 0, 0) * 0 })
        light({ color = colors[(i - 1) % #colors + 1], intensity = 6, range = 7, position = pos + vec3(0, 1.4, 0), shadows = i <= 4 })
    end
    camera.position = vec3(0, 4, 12)
    camera:look_at(vec3(0, 0.5, 0))
end

function update(dt)
    local a = time.elapsed * 0.2
    camera.position = vec3(math.sin(a) * 12, 4, math.cos(a) * 12)
    camera:look_at(vec3(0, 0.5, 0))
end