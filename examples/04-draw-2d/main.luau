-- A 2D game with draw.* only: catch the falling coins with the paddle (arrows / A D / left stick).
local paddle_x, score, coins, t = 480, 0, {}, 0

function start() math.randomseed(7) end

function update(dt)
    local dir = (input.down("right") and 1 or 0) - (input.down("left") and 1 or 0) + (input.down("d") and 1 or 0) - (input.down("a") and 1 or 0) + input.pad_axis("left_x")
    paddle_x = math.max(60, math.min(screen.width - 60, paddle_x + dir * 520 * dt))
    t += dt
    if t > 0.6 then
        t = 0
        table.insert(coins, { x = math.random(30, screen.width - 30), y = -10, v = math.random(160, 260) })
    end
    for i = #coins, 1, -1 do
        local c = coins[i]
        c.y += c.v * dt
        if c.y > screen.height - 40 and math.abs(c.x - paddle_x) < 70 then
            score += 1
            table.remove(coins, i)
        elseif c.y > screen.height + 20 then
            table.remove(coins, i)
        end
    end
end

function render()
    draw.rect(0, 0, screen.width, screen.height, "#14182a")
    for _, c in coins do draw.circle(c.x, c.y, 12, "#ffd23f") end
    draw.rect(paddle_x, screen.height - 30, 120, 16, "#4ac6ff", { align = "center" })
    draw.text("Coins: " .. score, 16, 16, { size = 28, shadow = true })
end