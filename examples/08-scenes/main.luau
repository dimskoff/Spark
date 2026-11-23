-- Scenes and prefabs: load a level file, spawn instances of a prefab, save the result.
-- Space: another door. S: write the scene to saved/arena.scene (each door is saved as the prefab plus what you changed).
local doors = 1
local message = ""

function start()
    scene.load("levels/arena.scene", { replace = true })    -- the level, its sun and its background
    camera.position = vec3(0, 5, 12)
    camera:look_at(vec3(0, 1, 0))

    local door = spawn("prefabs/door.prefab", { name = "Door B", position = vec3(-2, 0, -2), rotation = vec3(0, 0.4, 0) })
    door:find("Leaf").color = "#3a7bd5"                      -- a change to a prefab's object: saved as an override
end

function update(dt)
    if input.pressed("space") then
        doors = doors + 1
        spawn("prefabs/door.prefab", { name = "Door " .. doors, position = vec3(math.random(-8, 8), 0, math.random(-6, 0)) })
    end
    if input.pressed("s") then
        local warnings = scene.save("saved/arena.scene")
        message = "saved to saved/arena.scene" .. (#warnings > 0 and (" (" .. #warnings .. " warnings)") or "")
    end
end

function render()
    draw.text("Space: spawn a door    S: save the scene", 16, 16, { size = 20, shadow = true })
    draw.text(message, 16, 44, { size = 18, shadow = true })
end
