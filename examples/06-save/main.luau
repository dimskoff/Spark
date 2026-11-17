-- Persistent data: the click count survives restarts (stored in the user's data folder, not in the game folder).
local clicks

function start()
    clicks = save.get("clicks", 0)
end

function update(dt)
    if input.mouse_pressed("left") or input.pressed("space") then
        clicks += 1
        save.set("clicks", clicks)
    end
    if input.pressed("r") then clicks = 0 save.set("clicks", 0) end
end

function render()
    draw.rect(0, 0, screen.width, screen.height, "#101820")
    draw.text("Clicks: " .. clicks, screen.width / 2, screen.height / 2, { size = 64, align = "center", shadow = true })
    draw.text("Click or Space: +1   R: reset   (saved automatically)", screen.width / 2, screen.height - 40, { size = 20, align = "center", color = "#aab" })
end