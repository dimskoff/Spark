-- A game with a scenario test (tests/door.luau): run `spark test examples/10-scenario-test`.
-- Walk into the door zone (E) and it opens.
local door, opened = nil, false

function start()
    sun(vec3(-0.4, -1, -0.3), "white", 1.2)
    spawn(Mesh.plane(20), "#444")
    door = spawn(Mesh.box(2, 3, 0.2), "#8b5a2b", { name = "door", position = vec3(0, 1.5, -3) })
    camera.position = vec3(0, 3, 6)
    camera:look_at(vec3(0, 1.5, -3))
end

function update(dt)
    if input.pressed("e") and not opened then
        opened = true
        tween(door, { position = vec3(3, 1.5, -3) }, 0.5, "out")
    end
end

function render()
    draw.text(opened and "Door open" or "Press E", 16, 16, { size = 28, shadow = true })
end